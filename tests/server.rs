//! Le service HTTP de bout en bout, avec de vraies sockets (port éphémère, capteurs de démo).

use rpimon::collector::{Key, Monitor};
use rpimon::sensors::demo::DemoSensors;
use rpimon::server::config::Config;
use rpimon::server::{self, Running};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn config(port: u16, no_auth: bool) -> Config {
    Config { addr: SocketAddr::from(([127, 0, 0, 1], port)), demo: true, no_auth, password_file: None, data_dir: None }
}

fn start(monitor: &Monitor, password: Option<&str>) -> Running {
    server::start(&config(0, password.is_none()), monitor.history(), password.map(String::from)).expect("démarrage")
}

/// Envoie une requête HTTP/1.1 et renvoie (statut, en-têtes, corps).
fn http(addr: SocketAddr, method: &str, path: &str, headers: &[&str], body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let extra: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n{body}", body.len()).unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_string(), body.to_string())
}

#[test]
fn serves_health_with_security_headers() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, None);
    let (status, head, body) = http(srv.addr(), "GET", "/api/v1/health", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("\"status\":\"ok\""));
    let lower = head.to_lowercase();
    assert!(lower.contains("x-content-type-options: nosniff") && lower.contains("cache-control: no-store"), "{head}");
    srv.stop();
}

#[test]
fn busy_port_gives_an_explicit_error() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let err = server::start(&config(port, true), m.history(), None).err().expect("doit échouer");
    assert!(err.contains(&port.to_string()) && err.contains("déjà utilisé"), "{err}");
}

#[test]
fn login_over_the_wire_sets_a_hardened_cookie_that_unlocks_the_api() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, Some("pw-de-test"));
    let a = srv.addr();
    assert_eq!(http(a, "GET", "/api/v1/metrics/current", &[], "").0, 401);
    assert_eq!(http(a, "POST", "/api/v1/login", &[], r#"{"password":"faux"}"#).0, 401);
    let (status, head, _) = http(a, "POST", "/api/v1/login", &["X-Forwarded-Proto: https"], r#"{"password":"pw-de-test"}"#);
    assert_eq!(status, 200);
    let set = head.lines().find(|l| l.to_lowercase().starts_with("set-cookie:")).expect("Set-Cookie");
    assert!(set.contains("HttpOnly") && set.contains("Secure") && set.contains("SameSite=Strict"), "{set}");
    let cookie = set.split_once(':').unwrap().1.trim().split(';').next().unwrap().to_string();
    let (status, _, body) = http(a, "GET", "/api/v1/metrics/current", &[&format!("Cookie: {cookie}")], "");
    assert_eq!(status, 200);
    assert!(body.contains("\"cpu\""));
    srv.stop();
}

#[test]
fn many_clients_at_once_all_get_answers() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, None);
    let a = srv.addr();
    let threads: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(move || (0..15).all(|_| http(a, "GET", "/api/v1/metrics/current", &[], "").0 == 200)))
        .collect();
    assert!(threads.into_iter().all(|t| t.join().unwrap()));
    srv.stop();
}

#[test]
fn a_stalled_client_does_not_block_others() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, None);
    let _stalled: Vec<TcpStream> = (0..10).map(|_| TcpStream::connect(srv.addr()).unwrap()).collect();
    let t = Instant::now();
    assert_eq!(http(srv.addr(), "GET", "/api/v1/health", &[], "").0, 200);
    assert!(t.elapsed() < Duration::from_secs(2));
    srv.stop();
}

#[test]
fn collection_runs_without_any_client_and_survives_server_stop() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, None);
    std::thread::sleep(Duration::from_millis(2300)); // aucun client connecté
    let n = m.history().lock().unwrap().archive_points(Key::Cpu, None).len();
    assert!(n >= 1, "la collecte doit alimenter l'historique sans client");
    srv.stop();
    let before = m.history().lock().unwrap().snapshot(Key::Temp).values.len();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(m.history().lock().unwrap().snapshot(Key::Temp).values.len() > before, "arrêter le serveur ne doit pas arrêter la collecte");
}

#[test]
fn stop_is_prompt_and_frees_the_port() {
    let m = Monitor::start(Box::new(DemoSensors::new(1)));
    let srv = start(&m, None);
    let addr = srv.addr();
    let t = Instant::now();
    srv.stop();
    assert!(t.elapsed() < Duration::from_secs(2), "arrêt trop lent : {:?}", t.elapsed());
    TcpListener::bind(addr).expect("le port doit être libéré");
}
