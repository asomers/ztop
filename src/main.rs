// vim: tw=80
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
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Cell, Row, Table},
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
    /// only display datasets that are at least 0.1% busy (unimplemented)
    #[structopt(short = "a")]
    auto: bool,
    /// display datasets no more than this many levels deep.
    #[structopt(short = "d")]
    depth: Option<NonZeroUsize>,
    /// display update interval, in seconds or with the specified unit
    #[structopt(short = "t", parse(try_from_str = Cli::duration_from_str))]
    time: Option<Duration>,
    /// Reverse the sort (unimplemented)
    #[structopt(short = "r")]
    reverse: bool,
    /// Sort by the named column.  The name should match the column header.
    /// (unimplemented)
    #[structopt(short = "s")]
    sort: Option<String>,
    /// Display these datasets and their children
    datasets: Vec<String>,
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

mod ui {
    use super::*;
    use tui::{
        backend::Backend,
        Frame
    };

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
}

// https://github.com/rust-lang/rust-clippy/issues/7483
#[allow(clippy::or_fun_call)]
fn main() -> Result<(), Box<dyn Error>> {
    let cli: Cli = Cli::from_args();
    let mut tick_rate = cli.time.unwrap_or(Duration::from_secs(1));
    let mut app = App::new(cli.datasets, cli.depth);
    let stdout = io::stdout().into_raw_mode()?;

    let stdout = MouseTerminal::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let stdin = io::stdin();
    let mut events = Events::new(stdin);

    terminal.clear()?;
    while !app.should_quit() {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        match events.poll(&tick_rate) {
            Some(Event::Tick) => {
                app.on_tick();
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
            Some(Event::Key(Key::Char('D'))) => {
                app.on_d(false);
            }
            Some(Event::Key(Key::Char('q'))) => {
                app.on_q();
            }
            Some(Event::Key(Key::Char('r'))) => {
                app.on_r();
            }
            Some(Event::Key(Key::Char('d'))) => {
                app.on_d(true);
            }
            // TODO: other keys
            // f for filter dialog
            // F to clear the filter
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
