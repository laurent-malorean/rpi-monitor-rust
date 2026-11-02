//! Service permanent : collecte + serveur HTTP + sauvegarde, sans interface terminal.

use crate::collector::{History, Monitor};
use crate::sensors::Sensors;
use crate::server::{self, auth, config::Config, Running};
use crate::snapshot::{self, Saver};
use std::net::SocketAddr;
use std::time::Duration;

/// Intervalle de sauvegarde (l'arrêt propre en fait une dernière).
pub const SAVE_EVERY: Duration = Duration::from_secs(10 * 60);

pub struct Service {
    monitor: Monitor,
    server: Running,
    saver: Option<Saver>,
}

impl Service {
    pub fn start(config: &Config, sensors: Box<dyn Sensors>) -> Result<Self, String> {
        let password = match (config.no_auth, &config.password_file) {
            (true, _) => None,
            (false, Some(f)) => Some(auth::load_password(f)?),
            (false, None) => return Err("aucun mot de passe configuré".into()),
        };
        let mut history = History::new();
        if let Some(dir) = &config.data_dir {
            match snapshot::load(&mut history, dir) {
                Ok(true) => eprintln!("historique restauré depuis {}", dir.display()),
                Ok(false) => {}
                Err(e) => eprintln!("{e}"), // une sauvegarde illisible n'empêche pas de démarrer
            }
        }
        let monitor = Monitor::start_with(sensors, history);
        let server = match server::start(config, monitor.history(), password) {
            Ok(s) => s,
            Err(e) => {
                monitor.stop();
                return Err(e);
            }
        };
        let saver = config.data_dir.clone().map(|dir| Saver::start(monitor.history(), dir, SAVE_EVERY));
        Ok(Self { monitor, server, saver })
    }

    pub fn addr(&self) -> SocketAddr {
        self.server.addr()
    }

    /// Arrêt propre : plus de requêtes, fin de la collecte, puis dernière sauvegarde.
    pub fn stop(self) {
        self.server.stop();
        self.monitor.stop();
        if let Some(s) = self.saver {
            s.stop();
        }
    }
}
