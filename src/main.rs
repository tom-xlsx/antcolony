mod palette;
mod sim;
mod ui;

use std::io::{self, stdout, BufWriter, Stdout};
use std::time::{Duration, Instant};

use clap::Parser;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

use sim::{Config, World};

/// An ant colony that lives in a terminal pane.
///
/// Keys: q quit · space pause · r reset · + / - speed · f drop food · h status line.
/// Click anywhere to drop food on the surface at that column.
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
    /// Food piles kept on the surface at once.
    #[arg(long, default_value_t = 3)]
    food: usize,
    /// Fixed population: stored food never turns into new ants (also stops budding).
    #[arg(long)]
    no_grow: bool,
    /// Don't let a big colony send out a party to open a new nest.
    #[arg(long)]
    no_bud: bool,
    /// Most nest entrances allowed at once.
    #[arg(long, default_value_t = 4)]
    max_nests: usize,
    /// No droughts: endless calm.
    #[arg(long)]
    no_drought: bool,
    /// With --headless: print one line per drought (for tuning).
    #[arg(long)]
    report: bool,
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
const FRAME: Duration = Duration::from_millis(40);

type Term = Terminal<CrosstermBackend<BufWriter<Stdout>>>;

/// Raw mode, alternate screen, mouse reporting, and a 1 MiB write buffer so each frame
/// reaches the terminal in one write instead of a thousand small ones.
fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(BufWriter::with_capacity(1 << 20, stdout()));
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    // No `terminal.clear()` here: it asks the terminal for the cursor position and bails
    // out if the reply is slow (zellij), and the first frame paints every cell anyway.
    Ok(terminal)
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen, Show);
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let cfg = Config {
        ants: args.ants,
        food_piles: args.food,
        grow: !args.no_grow,
        bud: !args.no_bud && !args.no_grow,
        max_nests: args.max_nests.max(1),
        drought: !args.no_drought,
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
        if args.report {
            for r in &world.reports {
                println!(
                    "drought at day {} for {} days: nests {} -> {}, ants {} -> {}",
                    r.started / sim::DAY,
                    (r.ended - r.started) / sim::DAY,
                    r.nests_before,
                    r.nests_after,
                    r.ants_before,
                    r.ants_after
                );
            }
        }
        return Ok(());
    }

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, cfg, args.speed, !args.quiet);
    restore_terminal();
    if let Err(e) = &result {
        eprintln!("antcolony: {e}");
    }
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
            "{} · ants {} · dug {} · stored {} · food {} · x{} ",
            w.nest_summary(),
            w.ants.len(),
            w.dug,
            w.stored(),
            w.food.len(),
            self.speed
        );
        if let Some(season) = w.season_label() {
            s.insert_str(0, &format!("{season} · "));
        }
        if self.paused {
            s.insert_str(0, "paused · ");
        }
        Some(s)
    }

    fn rebuild(&mut self, cols: u16, rows: u16) {
        self.world = World::new(self.cfg.clone(), cols as usize, rows as usize * 2);
    }
}

fn run(terminal: &mut Term, cfg: Config, speed: u8, show_status: bool) -> io::Result<()> {
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
        // Input the parser can't make sense of (odd terminal replies, exotic mouse modes)
        // comes back as an error from crossterm. That is never a reason to quit.
        if !event::poll(wait).unwrap_or(false) {
            continue;
        }
        let Ok(ev) = event::read() else { continue };
        match ev {
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
                    let x = fastrand::usize(..app.world.w);
                    app.world.add_food_at(x);
                }
                _ => {}
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                app.world.add_food_at(m.column as usize);
            }
            Event::Resize(w, h) => app.rebuild(w, h),
            _ => {}
        }
    }
}
