// vim: tw=80
use cfg_if::cfg_if;
use nix::{
    sys::time::TimeSpec,
    time::{ClockId, clock_gettime},
};
use std::{
    collections::{HashMap, hash_map},
    error::Error,
    io,
    mem,
    time::Duration
};
use termion::{
    event::Key,
    input::MouseTerminal,
    raw::IntoRawMode,
};
use tui::{
    backend::TermionBackend,
    layout::Constraint,
    style::{Color, Style},
    widgets::{Block, Cell, Row, Table},
    Terminal,
};

mod event;
use self::event::{Events, Event};

cfg_if! {
    if #[cfg(target_os = "freebsd")] {
        mod freebsd;
        use freebsd::{SnapshotIter};
    }
}


/// A snapshot in time of a dataset's statistics.
///
/// The various fields are not saved atomically, but ought to be close.
#[derive(Clone, Debug)]
pub struct Snapshot {
    name: String,
    nunlinked: u64,
    nunlinks: u64,
    nread: u64,
    reads: u64,
    nwritten: u64,
    writes: u64,
}

impl Snapshot {
    fn compute(&self, prev: Option<&Self>, etime: f64) -> Element {
        if let Some(prev) = prev {
            Element {
                name: self.name.clone(),
                ops_r: (self.reads - prev.reads ) as f64 / etime,
                r_s: (self.nread - prev.nread ) as f64 / etime,
                ops_w: (self.writes - prev.writes ) as f64 / etime,
                w_s: (self.nwritten - prev.nwritten ) as f64 / etime,
                ops_d: (self.nunlinks - prev.nunlinks ) as f64 / etime,
                d_s: (self.nunlinked - prev.nunlinked ) as f64 / etime,
            }
        } else {
            Element {
                name: self.name.clone(),
                ops_r: self.reads as f64 / etime,
                r_s: self.nread as f64 / etime,
                ops_w: self.writes as f64 / etime,
                w_s: self.nwritten as f64 / etime,
                ops_d: self.nunlinks as f64 / etime,
                d_s: self.nunlinked as f64 / etime,
            }
        }
    }

    /// Iterate through all ZFS datasets, returning stats for each.
    pub fn iter() -> Result<SnapshotIter, Box<dyn Error>> {
        SnapshotIter::new()
    }
}

/// One thing to display in the table
#[derive(Clone, Debug)]
struct Element {
    name: String,
    /// Read IOPs
    ops_r: f64,
    /// Read B/s
    r_s: f64,
    /// Delete IOPs
    ops_d: f64,
    /// Delete B/s
    d_s: f64,
    /// Write IOPs
    ops_w: f64,
    /// Write B/s
    w_s: f64,
}

#[derive(Default)]
struct DataSource {
    prev: HashMap<String, Snapshot>,
    prev_ts: Option<TimeSpec>,
    cur: HashMap<String, Snapshot>,
    cur_ts: Option<TimeSpec>,
}

impl DataSource {
    /// Iterate through all the datasets, returning current stats
    fn iter<'a>(&'a mut self) -> impl Iterator<Item=Element> + 'a {
        let etime = if let Some(prev_ts) = self.prev_ts.as_ref() {
            let delta = *self.cur_ts.as_ref().unwrap() - *prev_ts;
            delta.tv_sec() as f64 + delta.tv_nsec() as f64 * 1e-9
        } else {
            let boottime = clock_gettime(ClockId::CLOCK_UPTIME).unwrap();
            boottime.tv_sec() as f64 + boottime.tv_nsec() as f64 * 1e-9
        };
        DataSourceIter {
            inner_iter: self.cur.iter(),
            ds: self,
            etime
        }
    }

    fn refresh(&mut self) -> Result<(), Box<dyn Error>> {
        let now = clock_gettime(ClockId::CLOCK_MONOTONIC)?;
        self.prev = mem::replace(&mut self.cur, HashMap::new());
        self.prev_ts = self.cur_ts.replace(now);
        for rss in Snapshot::iter().unwrap() {
            let ss = rss?;
            self.cur.insert(ss.name.clone(), ss);
        }
        Ok(())
    }
}

struct DataSourceIter<'a> {
    inner_iter: hash_map::Iter<'a, String, Snapshot>,
    ds: &'a DataSource,
    etime: f64
}

impl<'a> Iterator for DataSourceIter<'a> {
    type Item = Element;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner_iter.next()
            .map(|(name, ss)| ss.compute(self.ds.prev.get(name), self.etime))
    }
}


pub struct App {
    data: DataSource,
    should_quit: bool
}

impl App {
    fn new() -> Self {
        let mut data = DataSource::default();
        data.refresh().unwrap();
        App {
            data,
            should_quit: false
        }
    }

    fn elements<'a>(&'a mut self) -> impl Iterator<Item=Element> + 'a {
        self.data.iter()
    }

    fn on_q(&mut self) {
        self.should_quit = true;
    }

    fn on_tick(&mut self) {
        self.data.refresh().unwrap();
    }
}

mod ui {
    use super::*;
    use tui::{
        backend::Backend,
        Frame
    };

    pub fn draw<B: Backend>(f: &mut Frame<B>, app: &mut App) {
        //for elem in app.elements() {
            //println!("{:40} {:>6.0} {:10} {:13} {:10} {:10} {:10}",
                     //elem.name,
                     //elem.r_s / 1024.0,
                     //elem.ops_r,
                     //elem.w_s,
                     //elem.ops_w,
                     //elem.d_s,
                     //elem.ops_d,
             //);
        //}
        let hstyle = Style::default().fg(Color::Red);
        let header = Row::new([
            Cell::from("Dataset").style(hstyle),
            Cell::from("r/s").style(hstyle),
            Cell::from("kB/s r").style(hstyle),
        ]).style(Style::default().bg(Color::Blue));;
        let rows = app.elements()
            .map(|elem| Row::new([
                Cell::from(elem.name),
                Cell::from(format!("{:>6.0}", elem.ops_r)),
                Cell::from(format!("{:>6.0}", elem.r_s / 1024.0)),
            ])).collect::<Vec<_>>();
        let widths = [
            Constraint::Length(40),
            Constraint::Length(7),
            Constraint::Length(7)
        ];
        let t = Table::new(rows)
            .header(header)
            .block(Block::default())
            .widths(&widths);
        f.render_widget(t, f.size());
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut app = App::new();
    let tick_rate: Duration = Duration::from_secs(1);
    let stdout = io::stdout().into_raw_mode()?;

    let stdout = MouseTerminal::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let stdin = io::stdin();
    let mut events = Events::new(stdin);

    terminal.clear()?;
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        match events.poll(&tick_rate) {
            Some(Event::Tick) => {
                app.on_tick();
            }
            Some(Event::Key(Key::Char('q'))) => {
                app.on_q();
            }
            _ => unimplemented!()
        }
    }
    Ok(())
}
