// vim: tw=80
use cfg_if::cfg_if;
use gumdrop::Options;
use nix::{
    sys::time::TimeSpec,
    time::{ClockId, clock_gettime},
};
use std::{
    collections::HashMap,
    error::Error,
    io,
    mem,
    num::NonZeroUsize,
    slice,
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


/// Display ZFS datasets' I/O in real time
// TODO: shorten the help options so they fit on 80 columns.
#[derive(Debug, Default, Options)]
struct Cli {
    #[options(help = "print help message")]
    help: bool,
    /// only display datasets that are at least 0.1% busy (unimplemented)
    #[options(short = 'a')]
    auto: bool,
    /// Display these datasets and their children
    #[options(free)]
    datasets: Vec<String>,
    /// display datasets no more than this many levels deep.
    depth: Option<NonZeroUsize>,
    /// display update interval, in seconds or with the specified unit
    #[options(short = 's')]
    // Note: argh has a "from_str_fn" property that could be used to create a
    // custom parser, to parse interval directly to an int or a Duration.  That
    // would make it easier to save the config file.  But gumpdrop doesn't have
    // that option.
    time: Option<String>,
    /// Reverse the sort (unimplemented)
    #[options(short = 'r')]
    reverse: bool,
    /// Sort by the named column.  The name should match the column header.
    /// (unimplemented)
    sort: Option<String>,
}

impl Cli {
    fn interval(&self) -> Result<Duration, humanize_rs::ParseError> {
        match self.time.as_ref() {
            None => Ok(Duration::from_secs(1)),
            Some(s) => {
                if let Ok(fsecs) = s.parse::<f64>() {
                    Ok(Duration::from_secs_f64(fsecs))
                } else {
                    // Must have units
                    humanize_rs::duration::parse(s)
                }
            }
        }
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
    cur: Vec<Snapshot>,
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
        self.prev = mem::replace(&mut self.cur, Vec::new())
            .into_iter()
            .map(|ss| (ss.name.clone(), ss))
            .collect();
        self.prev_ts = self.cur_ts.replace(now);
        for rss in Snapshot::iter().unwrap() {
            self.cur.push(rss?);
        }
        Ok(())
    }
}

struct DataSourceIter<'a> {
    inner_iter: slice::Iter<'a, Snapshot>,
    ds: &'a DataSource,
    etime: f64
}

impl<'a> Iterator for DataSourceIter<'a> {
    type Item = Element;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner_iter.next()
            .map(|ss| ss.compute(self.ds.prev.get(&ss.name), self.etime))
    }
}


#[derive(Default)]
pub struct App {
    data: DataSource,
    datasets: Vec<String>,
    depth: Option<NonZeroUsize>,
    should_quit: bool
}

impl App {
    fn new(cli: Cli) -> Self {
        let mut data = DataSource::default();
        data.refresh().unwrap();
        App {
            data,
            datasets: cli.datasets,
            depth: cli.depth,
            .. Default::default()
        }
    }

    /// Return the elements that should be displayed
    fn elements<'a>(&'a mut self) -> impl Iterator<Item=Element> + 'a {
        let datasets = self.datasets.clone();
        let depth = self.depth;
        self.data.iter()
            .filter(move |elem| {
                if let Some(limit) = depth {
                    let edepth = elem.name.split("/").count();
                    edepth <= limit.get()
                } else {
                    true
                }
            }).filter(move |elem|
                datasets.is_empty() ||
                    datasets.iter().any(|ds| elem.name.starts_with(ds))
            )
    }

    fn on_d(&mut self, more_depth: bool) {
        self.depth = if more_depth {
            match self.depth {
                None => NonZeroUsize::new(1),
                Some(x) => NonZeroUsize::new(x.get() + 1)
            }
        } else {
            match self.depth {
                None => None,
                Some(x) => NonZeroUsize::new(x.get() - 1)
            }
        }
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
        let hstyle = Style::default().fg(Color::Red);
        let header = Row::new([
            Cell::from("   r/s").style(hstyle),
            Cell::from(" kB/s r").style(hstyle),
            Cell::from("   w/s").style(hstyle),
            Cell::from(" kB/s w").style(hstyle),
            Cell::from("   d/s").style(hstyle),
            Cell::from("kB/s d").style(hstyle),
            Cell::from("Dataset").style(hstyle),
        ]).style(Style::default().bg(Color::Blue));
        let rows = app.elements()
            .map(|elem| Row::new([
                Cell::from(format!("{:>6.0}", elem.ops_r)),
                Cell::from(format!("{:>7.0}", elem.r_s / 1024.0)),
                Cell::from(format!("{:>6.0}", elem.ops_w)),
                Cell::from(format!("{:>7.0}", elem.w_s / 1024.0)),
                Cell::from(format!("{:>6.0}", elem.ops_d)),
                Cell::from(format!("{:>6.0}", elem.d_s / 1024.0)),
                Cell::from(elem.name),
            ])).collect::<Vec<_>>();
        let widths = [
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(40),
        ];
        let t = Table::new(rows)
            .header(header)
            .block(Block::default())
            .widths(&widths);
        f.render_widget(t, f.size());
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli: Cli = Cli::parse_args_default_or_exit();
    let mut tick_rate = cli.interval()?;
    let mut app = App::new(cli);
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
            Some(Event::Key(Key::Char('<'))) => {
                tick_rate /= 2;
            }
            Some(Event::Key(Key::Char('>'))) => {
                tick_rate *= 2;
            }
            Some(Event::Key(Key::Char('D'))) => {
                app.on_d(false);
            }
            Some(Event::Key(Key::Char('q'))) => {
                app.on_q();
            }
            Some(Event::Key(Key::Char('d'))) => {
                app.on_d(true);
            }
            // TODO: other keys
            // f for filter dialog
            // F to clear the filter
            // - and + to change the sort column
            // r to reverse the sort
            Some(Event::Key(_)) => {
                // Ignore unknown keys
            }
            None => {
                // stdin closed for some reason
                break;
            },
            _ => unimplemented!()
        }
    }
    terminal.set_cursor(0, terminal.size()?.height - 1)?;
    Ok(())
}
