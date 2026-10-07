mod palette;
mod sim;
mod ui;

use std::io::{self, stdout};
use std::time::{Duration, Instant};

use clap::Parser;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;

use sim::{Config, World};

/// An ant colony that lives in a terminal pane.
///
/// Keys: q quit · space pause · r reset · + / - speed · f drop food · h status line.
/// Click anywhere to drop food there.
#[derive(Parser, Debug)]
#[command(version, about, verbatim_doc_comment)]
struct Args {
    /// Starting ants (default scales with the pane size).
    #[arg(long)]
    ants: Option<usize>,
    /// Simulation speed, 1 (slowest) to 8.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=8))]
    speed: u8,
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

/// Ticks per frame at each speed level (index = level - 1). Level 1 also halves the frame rate.
const SPEEDS: [u32; 8] = [1, 1, 2, 3, 4, 6, 8, 12];
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

    // ratatui::init installs a panic hook that restores the terminal; wrap it so mouse
    // reporting is switched off too, or a crash leaves the pane spraying escape codes on clicks.
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));

    let result = run(&mut terminal, cfg, args.speed, !args.quiet);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

struct App {
    world: World,
    cfg: Config,
    speed: u8,
    paused: bool,
    show_status: bool,
}

impl App {
    fn status(&self) -> Option<String> {
        if !self.show_status {
            return None;
        }
        let w = &self.world;
        let mut s = format!(
            "nests {} · ants {} · food {} · delivered {} · x{} ",
            w.nests.len(),
            w.ants.len(),
            w.food.len(),
            w.delivered,
            self.speed
        );
        if self.paused {
            s.insert_str(0, "paused · ");
        }
        Some(s)
    }

    fn rebuild(&mut self, cols: u16, rows: u16) {
        self.world = World::new(self.cfg.clone(), cols as usize, rows as usize * 2);
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, cfg: Config, speed: u8, show_status: bool) -> io::Result<()> {
    let size = terminal.size()?;
    let mut app = App {
        world: World::new(cfg.clone(), size.width as usize, size.height as usize * 2),
        cfg,
        speed,
        paused: false,
        show_status,
    };
    let mut next_frame = Instant::now();

    loop {
        let now = Instant::now();
        if now >= next_frame {
            if !app.paused {
                for _ in 0..SPEEDS[app.speed as usize - 1] {
                    app.world.tick();
                }
            }
            terminal.draw(|frame| {
                let status = app.status();
                frame.render_widget(ui::Colony { world: &app.world, status }, frame.area());
            })?;
            next_frame = now + if app.speed == 1 { FRAME * 2 } else { FRAME };
        }
        let wait = next_frame.saturating_duration_since(Instant::now());
        if !event::poll(wait)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char(' ') => app.paused = !app.paused,
                KeyCode::Char('h') => app.show_status = !app.show_status,
                KeyCode::Char('+') | KeyCode::Char('=') => app.speed = (app.speed + 1).min(8),
                KeyCode::Char('-') => app.speed = (app.speed - 1).max(1),
                KeyCode::Char('r') => {
                    app.cfg.seed = fastrand::u64(..);
                    let size = terminal.size()?;
                    app.rebuild(size.width, size.height);
                }
                KeyCode::Char('f') => {
                    let (x, y) = (fastrand::f32() * app.world.w as f32, fastrand::f32() * app.world.h as f32);
                    app.world.add_food_at(x, y);
                }
                _ => {}
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                app.world.add_food_at(m.column as f32 + 0.5, m.row as f32 * 2.0 + 1.0);
            }
            Event::Resize(w, h) => app.rebuild(w, h),
            _ => {}
        }
    }
}
