//! Persistance de l'historique long : un fichier JSON écrit de façon atomique,
//! relu au démarrage pour survivre à un redémarrage du service.

use crate::archive::{round2, Point};
use crate::collector::{History, Key};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const FILE: &str = "history.json";
const VERSION: u64 = 1;

type Data = Vec<(Key, Vec<Point>)>;

fn write(data: &Data, dir: &Path) -> Result<(), String> {
    let mut series = serde_json::Map::new();
    for (key, points) in data {
        let rows: Vec<Value> = points.iter().map(|p| json!([p.ts, round2(p.mean), round2(p.min), round2(p.max)])).collect();
        series.insert(key.id().into(), Value::Array(rows));
    }
    let saved_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let text = json!({ "version": VERSION, "saved_at": saved_at, "series": series }).to_string();
    let (tmp, dest) = (dir.join(format!("{FILE}.tmp")), dir.join(FILE));
    let io = |e: std::io::Error| format!("sauvegarde impossible dans {} : {e}", dir.display());
    std::fs::write(&tmp, text).map_err(io)?;
    std::fs::rename(&tmp, &dest).map_err(io) // atomique : jamais de fichier à moitié écrit
}

/// Écrit l'historique long dans `dir/history.json`.
pub fn save(history: &History, dir: &Path) -> Result<(), String> {
    write(&history.archive_export(), dir)
}

fn parse(text: &str) -> Option<Data> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v["version"].as_u64()? != VERSION {
        return None;
    }
    let series = v["series"].as_object()?;
    let mut out = vec![];
    for key in Key::ALL {
        let Some(rows) = series.get(key.id()) else { continue };
        let mut points = vec![];
        for r in rows.as_array()? {
            let r = r.as_array()?;
            let n = |i: usize| r.get(i).and_then(Value::as_f64);
            points.push(Point { ts: n(0)?, mean: n(1)?, min: n(2)?, max: n(3)? });
        }
        out.push((key, points));
    }
    Some(out)
}

/// Recharge la sauvegarde. `Ok(false)` : pas de fichier (premier démarrage).
/// Une sauvegarde illisible est une erreur et laisse l'historique intact.
pub fn load(history: &mut History, dir: &Path) -> Result<bool, String> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("lecture de {} impossible : {e}", path.display())),
    };
    let data = parse(&text).ok_or_else(|| format!("{} illisible ou d'une version inconnue : ignoré", path.display()))?;
    history.archive_import(data);
    Ok(true)
}

/// Sauvegarde périodique en arrière-plan, plus une dernière à l'arrêt.
/// Une erreur d'écriture est journalisée mais n'arrête ni la collecte ni la sauvegarde suivante.
pub struct Saver {
    stop_tx: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Saver {
    pub fn start(history: Arc<Mutex<History>>, dir: std::path::PathBuf, every: Duration) -> Self {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let handle = std::thread::Builder::new()
            .name("snapshot".into())
            .spawn(move || loop {
                let stopping = !matches!(stop_rx.recv_timeout(every), Err(RecvTimeoutError::Timeout));
                let data = history.lock().unwrap_or_else(|e| e.into_inner()).archive_export();
                if let Err(e) = write(&data, &dir) {
                    eprintln!("{e}");
                }
                if stopping {
                    break;
                }
            })
            .ok();
        Self { stop_tx: Some(stop_tx), handle }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop_tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::{History, Key, Readings};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rpimon-snap-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn history_with_data() -> History {
        let mut h = History::new();
        for t in 0..35 {
            h.record_slow(1000.0 + t as f64, &Readings { cpu: Some(10.0 + t as f64 / 3.0), temp: Some(45.5), ..Default::default() });
        }
        h
    }

    #[test]
    fn save_then_load_restores_the_points_of_every_indicator() {
        let d = dir("roundtrip");
        let mut h = history_with_data();
        save(&h, &d).unwrap();
        let mut fresh = History::new();
        assert_eq!(load(&mut fresh, &d), Ok(true));
        for key in Key::ALL {
            let expected: Vec<_> = h.archive_points(key, None).into_iter().filter(|p| p.ts < 1030.0).collect();
            let got = fresh.archive_points(key, None);
            assert_eq!(got.len(), expected.len(), "{key:?}");
            for (a, b) in got.iter().zip(&expected) {
                assert_eq!(a.ts, b.ts);
                assert!((a.mean - b.mean).abs() < 0.01 && (a.max - b.max).abs() < 0.01);
            }
        }
        h.reset();
    }

    #[test]
    fn missing_file_is_a_normal_first_start() {
        let mut h = History::new();
        assert_eq!(load(&mut h, &dir("missing")), Ok(false));
        assert!(h.archive_points(Key::Cpu, None).is_empty());
    }

    #[test]
    fn corrupt_file_is_an_error_and_leaves_the_history_untouched() {
        let d = dir("corrupt");
        std::fs::write(d.join(FILE), "{ pas du json").unwrap();
        let mut h = History::new();
        assert!(load(&mut h, &d).unwrap_err().contains(FILE));
        std::fs::write(d.join(FILE), r#"{"version":99,"series":{}}"#).unwrap();
        assert!(load(&mut h, &d).is_err());
        assert!(h.archive_points(Key::Cpu, None).is_empty());
    }

    #[test]
    fn save_is_atomic_and_leaves_no_temporary_file() {
        let d = dir("atomic");
        save(&history_with_data(), &d).unwrap();
        save(&history_with_data(), &d).unwrap();
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, [FILE]);
    }

    #[test]
    fn save_failure_is_reported_not_fatal() {
        let h = history_with_data();
        let err = save(&h, std::path::Path::new("/nonexistent/dir")).unwrap_err();
        assert!(err.contains("/nonexistent/dir"));
        assert_eq!(h.archive_points(Key::Cpu, None).len(), 4); // la mémoire n'est pas affectée
    }

    #[test]
    fn restored_history_keeps_growing_after_the_gap_of_the_restart() {
        let d = dir("gap");
        save(&history_with_data(), &d).unwrap();
        let mut h = History::new();
        load(&mut h, &d).unwrap();
        h.record_slow(5000.0, &Readings { cpu: Some(1.0), ..Default::default() });
        let ts: Vec<f64> = h.archive_points(Key::Cpu, None).iter().map(|p| p.ts).collect();
        assert_eq!(ts, [1000.0, 1010.0, 1020.0, 5000.0]); // le trou reste un trou, pas des zéros
    }

    #[test]
    fn saver_writes_periodically_and_once_more_at_stop() {
        let d = dir("saver");
        let h = Arc::new(Mutex::new(history_with_data()));
        let saver = Saver::start(h.clone(), d.clone(), Duration::from_millis(40));
        std::thread::sleep(Duration::from_millis(150));
        assert!(d.join(FILE).exists(), "écriture périodique");
        h.lock().unwrap().record_slow(9000.0, &Readings { cpu: Some(77.0), ..Default::default() });
        h.lock().unwrap().record_slow(9010.0, &Readings { cpu: Some(78.0), ..Default::default() });
        saver.stop(); // sauvegarde finale
        let mut fresh = History::new();
        load(&mut fresh, &d).unwrap();
        assert!(fresh.archive_points(Key::Cpu, None).iter().any(|p| p.ts == 9000.0));
    }

    #[test]
    fn saver_survives_write_errors_and_recovers() {
        let d = dir("recover").join("later");
        let h = Arc::new(Mutex::new(history_with_data()));
        let saver = Saver::start(h.clone(), d.clone(), Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(100)); // dossier absent : erreurs, mais pas de panique
        assert_eq!(h.lock().unwrap().archive_points(Key::Cpu, None).len(), 4);
        std::fs::create_dir_all(&d).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        saver.stop();
        assert!(d.join(FILE).exists());
    }

    #[test]
    fn saver_stop_is_prompt_even_with_a_long_interval() {
        let d = dir("prompt");
        let saver = Saver::start(Arc::new(Mutex::new(History::new())), d, Duration::from_secs(600));
        let t = std::time::Instant::now();
        saver.stop();
        assert!(t.elapsed() < Duration::from_secs(1));
    }
}
