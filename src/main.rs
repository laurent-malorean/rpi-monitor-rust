//! Dashboard TUI de monitoring Raspberry Pi 5 (lecture seule).
//!
//! Lancement : `rpi-monitor [--demo]` (démo automatique hors Linux).

use rpimon::app::App;
use rpimon::cli::{self, Command, ServeOptions, USAGE};
use rpimon::collector::Monitor;
use rpimon::sensors::{demo::DemoSensors, linux::LinuxSensors, Sensors};
use rpimon::server::config;
use rpimon::service::Service;
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
        Ok(Command::Serve(opts)) => serve(opts),
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn make_sensors(demo: bool) -> Box<dyn Sensors> {
    if demo {
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Box::new(DemoSensors::new(seed))
    } else {
        Box::new(LinuxSensors::new())
    }
}

/// Service permanent : tourne jusqu'à SIGINT/SIGTERM, journalise sur stderr (journald).
fn serve(opts: ServeOptions) -> ExitCode {
    let base = std::env::current_exe().ok().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).unwrap_or_else(|| ".".into());
    let config = match config::resolve(opts, &|k| std::env::var(k).ok(), &base) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration : {e}");
            return ExitCode::from(2);
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = ctrlc::set_handler(move || {
        let _ = tx.send(());
    }) {
        eprintln!("gestion des signaux impossible : {e}");
        return ExitCode::FAILURE;
    }
    let service = match Service::start(&config, make_sensors(config.demo)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("démarrage impossible : {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("rpi-monitor : à l'écoute sur http://{}{}", service.addr(), if config.demo { " (mode démo)" } else { "" });
    if config.no_auth {
        eprintln!("rpi-monitor : ATTENTION, authentification désactivée (--no-auth)");
    }
    let _ = rx.recv();
    eprintln!("rpi-monitor : arrêt en cours");
    service.stop();
    ExitCode::SUCCESS
}

fn run(demo: bool) -> ExitCode {
    let sensors = make_sensors(demo);
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
