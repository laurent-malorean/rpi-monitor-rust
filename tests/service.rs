//! Cycle de vie du service permanent : démarrage, restauration, arrêt propre.

use rpimon::collector::{History, Key, Readings};
use rpimon::sensors::demo::DemoSensors;
use rpimon::server::config::Config;
use rpimon::service::Service;
use rpimon::snapshot;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rpimon-svc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(data_dir: Option<&Path>) -> Config {
    Config {
        addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        demo: true,
        no_auth: true,
        password_file: None,
        data_dir: data_dir.map(Path::to_path_buf),
    }
}

fn get(addr: SocketAddr, path: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    raw.split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_string())
}

fn seeded(d: &Path) {
    let mut h = History::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    for t in 0..30 {
        h.record_slow(now - 3600.0 + t as f64, &Readings { cpu: Some(42.0), ..Default::default() });
    }
    snapshot::save(&h, d).unwrap();
}

#[test]
fn history_survives_a_restart() {
    let d = dir("restart");
    seeded(&d);
    let svc = Service::start(&config(Some(&d)), Box::new(DemoSensors::new(1))).unwrap();
    let body = get(svc.addr(), "/api/v1/metrics/history?key=cpu&period=7200");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(!v["series"]["cpu"].as_array().unwrap().is_empty(), "{body}");
    svc.stop();
}

#[test]
fn stop_writes_a_final_snapshot_with_the_collected_data() {
    let d = dir("final");
    let svc = Service::start(&config(Some(&d)), Box::new(DemoSensors::new(1))).unwrap();
    std::thread::sleep(Duration::from_millis(1300));
    svc.stop();
    assert!(d.join(snapshot::FILE).exists());
    let mut h = History::new();
    assert_eq!(snapshot::load(&mut h, &d), Ok(true));
}

#[test]
fn corrupt_snapshot_does_not_prevent_startup() {
    let d = dir("corrupt");
    std::fs::write(d.join(snapshot::FILE), "garbage").unwrap();
    let svc = Service::start(&config(Some(&d)), Box::new(DemoSensors::new(1))).unwrap();
    assert!(get(svc.addr(), "/api/v1/health").contains("ok"));
    svc.stop();
}

#[test]
fn unwritable_data_dir_does_not_stop_monitoring() {
    let svc = Service::start(&config(Some(Path::new("/nonexistent/data"))), Box::new(DemoSensors::new(1))).unwrap();
    std::thread::sleep(Duration::from_millis(1300));
    let body = get(svc.addr(), "/api/v1/metrics/current");
    assert!(body.contains("\"cpu\""), "{body}");
    svc.stop();
}

#[test]
fn missing_or_open_password_file_is_a_startup_error() {
    let d = dir("pw");
    let mut c = config(None);
    c.no_auth = false;
    c.password_file = Some(d.join("absent"));
    let e = Service::start(&c, Box::new(DemoSensors::new(1))).err().unwrap();
    assert!(e.contains("absent"), "{e}");
    let f = d.join("pw");
    std::fs::write(&f, "mot-de-passe-long").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    c.password_file = Some(f);
    assert!(Service::start(&c, Box::new(DemoSensors::new(1))).err().unwrap().contains("droits"));
}

#[test]
fn password_protects_the_running_service() {
    let d = dir("auth");
    let f = d.join("pw");
    std::fs::write(&f, "mot-de-passe-long\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut c = config(None);
    c.no_auth = false;
    c.password_file = Some(f);
    let svc = Service::start(&c, Box::new(DemoSensors::new(1))).unwrap();
    assert!(get(svc.addr(), "/api/v1/metrics/current").contains("authentification requise"));
    svc.stop();
}

#[test]
fn busy_port_is_reported_and_leaves_nothing_running() {
    let first = Service::start(&config(None), Box::new(DemoSensors::new(1))).unwrap();
    let mut c = config(None);
    c.addr = first.addr();
    let e = Service::start(&c, Box::new(DemoSensors::new(1))).err().unwrap();
    assert!(e.contains("déjà utilisé"), "{e}");
    first.stop();
    let _ = Key::Cpu;
}
