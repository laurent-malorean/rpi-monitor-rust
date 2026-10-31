//! Dashboard TUI de monitoring Raspberry Pi 5 (lecture seule).
//!
//! Lancement : `rpi-monitor [--demo]` (démo automatique hors Linux).

use rpimon::app::App;
use rpimon::cli::{self, Command, USAGE};
use rpimon::collector::Monitor;
use rpimon::sensors::{demo::DemoSensors, linux::LinuxSensors, Sensors};
use rpimon::ui;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REFRESH: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1), cfg!(target_os = "linux")) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Run { demo }) => run(demo),
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(demo: bool) -> ExitCode {
    let sensors: Box<dyn Sensors> = if demo {
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Box::new(DemoSensors::new(seed))
    } else {
        Box::new(LinuxSensors::new())
    };
    let monitor = Monitor::start(sensors);
    let mut app = App::new(monitor.history(), demo);

    let mut terminal = ratatui::init();
    silence_collector_panics();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    monitor.stop();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur terminal : {e}");
            ExitCode::FAILURE
        }
    }
}

/// Un capteur qui panique est rattrapé par le collecteur ; son message ne doit pas
/// s'afficher (ni restaurer le terminal) pendant que le TUI est actif.
fn silence_collector_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name() != Some("collector") {
            previous(info);
        }
    }));
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut next_refresh = Instant::now() + REFRESH;
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        let timeout = next_refresh.saturating_duration_since(Instant::now());
        if event::poll(timeout)? {
            if let Event::Key(k) = event::read()? {
                let ctrl_c = k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c');
                if ctrl_c {
                    return Ok(());
                }
                if k.kind == KeyEventKind::Press {
                    if let KeyCode::Char(c) = k.code {
                        app.on_key(c);
                    }
                }
            }
        }
        if Instant::now() >= next_refresh {
            app.refresh();
            next_refresh = Instant::now() + REFRESH;
        }
        if app.should_quit() {
            return Ok(());
        }
    }
}
