// vim: tw=80
use regex::Regex;
use structopt::StructOpt;
use std::{
    array,
    error::Error,
    io,
    num::NonZeroUsize,
    time::Duration
};
use termion::{
    event::Key,
    input::MouseTerminal,
    raw::IntoRawMode,
};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Terminal,
};

mod app;
use self::app::App;
mod event;
use self::event::{Events, Event};

/// Display ZFS datasets' I/O in real time
// TODO: shorten the help options so they fit on 80 columns.
#[derive(Debug, Default, StructOpt)]
struct Cli {
    /// only display datasets that have some activity.
    #[structopt(short = "a", long = "auto", verbatim_doc_comment)]
    auto: bool,
    /// Include child datasets' stats with their parents'.
    #[structopt(short = "c", long = "children")]
    children: bool,
    /// display datasets no more than this many levels deep.
    #[structopt(short = "d", long = "depth")]
    depth: Option<NonZeroUsize>,
    /// only display datasets with names matching filter, as a regex.
    #[structopt(short = "f", parse(try_from_str = Regex::new), long = "filter")]
    filter: Option<Regex>,
    /// display update interval, in seconds or with the specified unit
    #[structopt(short = "t", parse(try_from_str = Cli::duration_from_str),
        long = "time")]
    time: Option<Duration>,
    /// Reverse the sort (unimplemented)
    #[structopt(short = "r", long = "reverse")]
    reverse: bool,
    /// Sort by the named column.  The name should match the column header.
    /// (unimplemented)
    #[structopt(short = "s", long = "sort")]
    sort: Option<String>,
    /// Display these pools and their children
    pools: Vec<String>,
}

impl Cli {
    fn duration_from_str(s: &str) -> Result<Duration, humanize_rs::ParseError> {
        if let Ok(fsecs) = s.parse::<f64>() {
            Ok(Duration::from_secs_f64(fsecs))
        } else {
            // Must have units
            humanize_rs::duration::parse(s)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FilterPopup {
    new_regex: String
}

impl FilterPopup {
    pub fn on_enter(&mut self) -> Result<Regex, impl Error> {
        Regex::new(&self.new_regex)
    }

    pub fn on_key(&mut self, key: Key) {
        match key {
            Key::Char(c) => {
                self.new_regex.push(c);
            }
            Key::Backspace => {
                self.new_regex.pop();
            }
            _ => {}
        }
    }
}

mod ui {
    use super::*;
    use tui::{
        backend::Backend,
        Frame
    };

    // helper function to create a one-line popup box
    fn popup_layout(x: u16, y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Max(r.height.saturating_sub(y)/2),
                    Constraint::Length(y),
                    Constraint::Max(r.height.saturating_sub(y)/2),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Max(r.width.saturating_sub(x) / 2),
                    Constraint::Length(x),
                    Constraint::Max(r.width.saturating_sub(x) / 2),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }

    pub fn draw<B: Backend>(f: &mut Frame<B>, app: &mut App) {
        let hstyle = Style::default().fg(Color::Red);
        let sstyle = hstyle.add_modifier(Modifier::REVERSED);
        let hcells = array::IntoIter::new([
            Cell::from("   r/s"),
            Cell::from(" kB/s r"),
            Cell::from("   w/s"),
            Cell::from(" kB/s w"),
            Cell::from("   d/s"),
            Cell::from("kB/s d"),
            Cell::from("Dataset"),
        ]).enumerate()
            .map(|(i, cell)| {
                if Some(i) == app.sort_idx() {
                    cell.style(sstyle)
                } else {
                    cell.style(hstyle)
                }
            });
        let header = Row::new(hcells)
            .style(Style::default().bg(Color::Blue));
        let rows = app.elements()
            .into_iter()
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

    pub fn draw_filter<B: Backend>(f: &mut Frame<B>, app: &mut FilterPopup) {
        let area = popup_layout(40, 3, f.size());
        let popup_box = Paragraph::new(app.new_regex.as_ref())
            .block(
                Block::default()
                .borders(Borders::ALL)
                .title("Filter regex")
            );
        f.render_widget(Clear, area);
        f.render_widget(popup_box, area);
    }
}

// https://github.com/rust-lang/rust-clippy/issues/7483
#[allow(clippy::or_fun_call)]
fn main() -> Result<(), Box<dyn Error>> {
    let cli: Cli = Cli::from_args();
    let mut editting_filter = false;
    let mut tick_rate = cli.time.unwrap_or(Duration::from_secs(1));
    let mut app = App::new(cli.auto, cli.children, cli.pools, cli.depth,
                           cli.filter);
    let mut filter_popup = FilterPopup::default();
    let stdout = io::stdout().into_raw_mode()?;

    let stdout = MouseTerminal::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let stdin = io::stdin();
    let mut events = Events::new(stdin);

    terminal.clear()?;
    while !app.should_quit() {
        terminal.draw(|f| {
            ui::draw(f, &mut app);
            if editting_filter {
                ui::draw_filter(f, &mut filter_popup)
             }
        })?;

        match events.poll(&tick_rate) {
            Some(Event::Tick) => {
                app.on_tick();
            }
            Some(Event::Key(Key::Esc)) if editting_filter => {
                editting_filter = false;
            }
            Some(Event::Key(Key::Char('\n'))) if editting_filter => {
                let filter = filter_popup.on_enter()?;
                app.set_filter(filter);
                editting_filter = false;
            }
            Some(Event::Key(key)) if editting_filter => {
                filter_popup.on_key(key);
            }
            Some(Event::Key(Key::Char('+'))) => {
                app.on_plus();
            }
            Some(Event::Key(Key::Char('-'))) => {
                app.on_minus();
            }
            Some(Event::Key(Key::Char('<'))) => {
                tick_rate /= 2;
            }
            Some(Event::Key(Key::Char('>'))) => {
                tick_rate *= 2;
            }
            Some(Event::Key(Key::Char('a'))) => {
                app.on_a();
            }
            Some(Event::Key(Key::Char('c'))) => {
                app.on_c()?;
            }
            Some(Event::Key(Key::Char('D'))) => {
                app.on_d(false);
            }
            Some(Event::Key(Key::Char('d'))) => {
                app.on_d(true);
            }
            Some(Event::Key(Key::Char('F'))) => {
                app.clear_filter();
            }
            Some(Event::Key(Key::Char('f'))) => {
                editting_filter = true;
            }
            Some(Event::Key(Key::Char('q'))) => {
                app.on_q();
            }
            Some(Event::Key(Key::Char('r'))) => {
                app.on_r();
            }
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
