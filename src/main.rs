mod palette;
mod sim;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use sim::{Config, World};

/// An ant colony that lives in a terminal pane.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Starting ants (default scales with the pane size).
    #[arg(long)]
    ants: Option<usize>,
    /// Random seed. Same seed and pane size gives the same colony.
    #[arg(long)]
    seed: Option<u64>,
    /// Hide the status line.
    #[arg(long, short)]
    quiet: bool,
}

const FRAME: Duration = Duration::from_millis(33);

fn main() -> io::Result<()> {
    let args = Args::parse();
    let cfg = Config {
        ants: args.ants,
        seed: args.seed.unwrap_or_else(|| fastrand::u64(..)),
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, cfg, !args.quiet);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, cfg: Config, mut show_status: bool) -> io::Result<()> {
    let size = terminal.size()?;
    let mut world = World::new(cfg.clone(), size.width as usize, size.height as usize * 2);
    let mut next_frame = Instant::now();

    loop {
        let now = Instant::now();
        if now >= next_frame {
            world.tick();
            terminal.draw(|frame| {
                let status = show_status.then(|| format!("ants {} ", world.ants.len()));
                frame.render_widget(ui::Colony { world: &world, status }, frame.area());
            })?;
            next_frame = now + FRAME;
        }
        let wait = next_frame.saturating_duration_since(Instant::now());
        if !event::poll(wait)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char('h') => show_status = !show_status,
                _ => {}
            },
            Event::Resize(w, h) => {
                world = World::new(cfg.clone(), w as usize, h as usize * 2);
            }
            _ => {}
        }
    }
}
