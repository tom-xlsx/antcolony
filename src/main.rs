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
    /// Food piles kept in the world at once.
    #[arg(long, default_value_t = 3)]
    food: usize,
    /// Don't spawn new ants as food comes in (also stops nests growing and budding).
    #[arg(long)]
    no_grow: bool,
    /// Don't let crowded nests send out founding parties.
    #[arg(long)]
    no_bud: bool,
    /// Most nests allowed at once.
    #[arg(long, default_value_t = 4)]
    max_nests: usize,
    /// Hide the status line.
    #[arg(long, short)]
    quiet: bool,
    /// Run this many ticks with no terminal, print a text snapshot and stats, and exit.
    #[arg(long, value_name = "TICKS")]
    headless: Option<u64>,
    /// World size for --headless as COLSxROWS (terminal cells).
    #[arg(long, default_value = "71x24", value_parser = parse_size)]
    size: (usize, usize),
}

fn parse_size(s: &str) -> Result<(usize, usize), String> {
    let (w, h) = s.split_once('x').ok_or("expected COLSxROWS, e.g. 71x24")?;
    let p = |v: &str| v.trim().parse::<usize>().map_err(|e| e.to_string());
    Ok((p(w)?, p(h)?))
}

const FRAME: Duration = Duration::from_millis(33);

fn main() -> io::Result<()> {
    let args = Args::parse();
    let cfg = Config {
        ants: args.ants,
        food_piles: args.food,
        grow: !args.no_grow,
        bud: !args.no_bud && !args.no_grow,
        max_nests: args.max_nests.max(1),
        seed: args.seed.unwrap_or_else(|| fastrand::u64(..)),
        ..Config::default()
    };

    if let Some(ticks) = args.headless {
        let mut world = World::new(cfg, args.size.0, args.size.1 * 2);
        for _ in 0..ticks {
            world.tick();
        }
        print!("{}", world.snapshot());
        println!("{}", world.stats());
        return Ok(());
    }

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
                let status = show_status.then(|| format!("{} ", world.stats()));
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
                KeyCode::Char('f') => {
                    let (x, y) = (fastrand::f32() * world.w as f32, fastrand::f32() * world.h as f32);
                    world.add_food_at(x, y);
                }
                _ => {}
            },
            Event::Resize(w, h) => {
                world = World::new(cfg.clone(), w as usize, h as usize * 2);
            }
            _ => {}
        }
    }
}
