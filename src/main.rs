mod palette;
mod sim;
mod ui;

use std::io::{self, stdout, Write};
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
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Clear, Paragraph};
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
/// How long the post-drought notice stays up.
const NOTICE: Duration = Duration::from_secs(8);

/// Collects a whole frame, then hands it to the terminal in modest chunks with a breath
/// between them. One huge write (a full repaint is a few hundred kilobytes) can tear on
/// the way through a multiplexer and the terminal, which shows up as striped rows and
/// missing letters; paced out, it arrives intact.
struct Paced {
    buf: Vec<u8>,
}

const CHUNK: usize = 16 * 1024;

impl Write for Paced {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut out = stdout().lock();
        let big = self.buf.len() > 2 * CHUNK;
        for chunk in self.buf.chunks(CHUNK) {
            out.write_all(chunk)?;
            out.flush()?;
            if big {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        self.buf.clear();
        Ok(())
    }
}

type Term = Terminal<CrosstermBackend<Paced>>;

/// Raw mode, alternate screen, mouse reporting, and a paced writer so each frame reaches
/// the terminal whole.
fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(Paced { buf: Vec::with_capacity(1 << 20) });
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
        for line in world.nest_lines() {
            println!("{line}");
        }
        if args.report {
            for r in &world.reports {
                let rebuilt = match r.rebuilt_days {
                    Some(d) => format!("rebuilt in {d} days"),
                    None => "not rebuilt before the next dry spell".to_string(),
                };
                println!(
                    "drought at day {} for {} days: nests {} -> {}, ants {} -> {}, stored {} -> {}, lost {} ants {} queens, {rebuilt}",
                    r.started / sim::DAY,
                    (r.ended - r.started) / sim::DAY,
                    r.nests_before,
                    r.nests_after,
                    r.ants_before,
                    r.ants_after,
                    r.stored_before,
                    r.stored_after,
                    r.ants_lost,
                    r.queens_lost,
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
    /// Drought reports already announced.
    reports_seen: usize,
    /// The notice on screen and when it went up.
    notice: Option<(Vec<String>, Instant)>,
    /// Season tint currently shown, the one dissolving out, and the dissolve progress.
    shown_season: sim::Season,
    fading_from: Option<sim::Season>,
    fade: f32,
}

/// Dissolve step per frame: about two seconds for a full change of weather.
const FADE_STEP: f32 = 0.02;

impl App {
    /// Status line, dropping the least important parts until it fits the pane.
    fn status(&self, width: usize) -> Option<String> {
        if !self.show_status {
            return None;
        }
        let w = &self.world;
        // Parts in order of importance (most important first).
        let mut parts: Vec<(u8, String)> = vec![
            (0, format!("x{}", self.speed)),
            (1, w.nest_summary()),
            (2, format!("ants {}", w.ants.len())),
            (3, format!("food {}", w.food.len())),
            (4, w.job_summary()),
            (5, format!("stored {}", w.stored())),
            (6, format!("eggs {}", w.eggs.len())),
        ];
        if let Some(season) = w.season_label() {
            parts.push((0, season));
        }
        if self.paused {
            parts.push((0, "paused".into()));
        }
        // Display order: paused, season, nests, ants, jobs, eggs, stored, food, speed.
        let order = |p: &(u8, String)| match p.1.as_str() {
            "paused" => 0,
            _ if p.0 == 0 && p.1.starts_with('x') => 9,
            _ if p.0 == 0 => 1,
            _ => p.0 + 1,
        };
        loop {
            let mut shown: Vec<&(u8, String)> = parts.iter().collect();
            shown.sort_by_key(|p| order(p));
            let line: String = shown.iter().map(|p| p.1.as_str()).collect::<Vec<_>>().join(" \u{b7} ") + " ";
            if line.chars().count() <= width || parts.len() <= 3 {
                return Some(line);
            }
            // Drop the least important optional part.
            let drop = parts.iter().enumerate().filter(|(_, p)| p.0 > 0).max_by_key(|(_, p)| p.0).map(|(i, _)| i);
            match drop {
                Some(i) => {
                    parts.remove(i);
                }
                None => return Some(line),
            }
        }
    }

    fn rebuild(&mut self, cols: u16, rows: u16) {
        self.world = World::new(self.cfg.clone(), cols as usize, rows as usize * 2);
        self.reports_seen = 0;
        self.notice = None;
        self.shown_season = self.world.season;
        self.fading_from = None;
        self.fade = 1.0;
    }

    /// Advance the weather dissolve, starting a new one when the season's tint changes.
    fn advance_fade(&mut self) {
        if ui::weather_key(self.world.season) != ui::weather_key(self.shown_season) {
            self.fading_from = Some(self.shown_season);
            self.shown_season = self.world.season;
            self.fade = 0.0;
        }
        if self.fading_from.is_some() {
            self.fade += FADE_STEP;
            if self.fade >= 1.0 {
                self.fading_from = None;
                self.fade = 1.0;
            }
        }
    }

    /// When a drought has just ended, put up a little box with the toll.
    fn check_notice(&mut self) {
        if self.world.reports.len() <= self.reports_seen {
            return;
        }
        self.reports_seen = self.world.reports.len();
        let r = self.world.reports.last().unwrap();
        let days = (r.ended - r.started) / sim::DAY;
        let plural = |n: u64, one: &str, many: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") };
        let mut lines = vec![format!("Drought over after {}", plural(days, "day", "days"))];
        lines.push(format!("{} and {} lost", plural(r.ants_lost, "ant", "ants"), plural(r.queens_lost, "queen", "queens")));
        if r.nests_after < r.nests_before {
            lines.push(format!("{} of {} nests survived", r.nests_after, r.nests_before));
        }
        self.notice = Some((lines, Instant::now()));
    }
}

/// The notice box, centred near the top of the pane.
fn draw_notice(frame: &mut ratatui::Frame, lines: &[String]) {
    let area = frame.area();
    let width = (lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16 + 4).min(area.width);
    let height = lines.len() as u16 + 2;
    if area.height < height + 2 || area.width < width {
        return;
    }
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + 1, width, height);
    let style = Style::default()
        .fg(ratatui::style::Color::Rgb(230, 220, 200))
        .bg(ratatui::style::Color::Rgb(40, 30, 22));
    frame.render_widget(Clear, rect);
    let text: Vec<ratatui::text::Line> = lines.iter().map(|l| ratatui::text::Line::from(l.as_str())).collect();
    frame.render_widget(
        Paragraph::new(text).style(style).centered().block(Block::bordered().style(style)),
        rect,
    );
}

fn run(terminal: &mut Term, cfg: Config, speed: u8, show_status: bool) -> io::Result<()> {
    let size = terminal.size()?;
    let mut app = App {
        world: World::new(cfg.clone(), size.width as usize, size.height as usize * 2),
        cfg,
        speed,
        paused: false,
        show_status,
        reports_seen: 0,
        notice: None,
        shown_season: sim::Season::Calm { until: 0 },
        fading_from: None,
        fade: 1.0,
    };
    app.shown_season = app.world.season;
    let mut next_frame = Instant::now();

    loop {
        let now = Instant::now();
        if now >= next_frame {
            if !app.paused {
                for _ in 0..SPEEDS[app.speed as usize - 1] {
                    app.world.tick();
                }
                app.check_notice();
            }
            if app.notice.as_ref().is_some_and(|(_, since)| since.elapsed() > NOTICE) {
                app.notice = None;
            }
            app.advance_fade();
            terminal.draw(|frame| {
                let status = app.status(frame.area().width as usize);
                frame.render_widget(
                    ui::Colony { world: &app.world, status, fading_from: app.fading_from, fade: app.fade },
                    frame.area(),
                );
                if let Some((lines, _)) = &app.notice {
                    draw_notice(frame, lines);
                }
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
