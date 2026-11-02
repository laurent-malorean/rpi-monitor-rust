//! Service permanent : configuration, API HTTP, authentification.

pub mod api;
pub mod auth;
pub mod config;

use crate::collector::History;
use api::{Info, Response};
use auth::{Auth, Login};
use serde_json::json;
use config::Config;
use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Server};

const INDEX_HTML: &str = include_str!("../../web/index.html");
const STYLES_CSS: &str = include_str!("../../web/styles.css");
const APP_JS: &str = include_str!("../../web/app.js");
const MAX_BODY: usize = 1024;

/// Requête HTTP réduite à ce dont le routage a besoin.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub cookie: Option<&'a str>,
    pub body: &'a [u8],
    pub https: bool, // requête relayée par un tunnel HTTPS (X-Forwarded-Proto)
}

pub struct Shared {
    pub history: Arc<Mutex<History>>,
    pub info: Info,
    pub auth: Auth,
}

fn plain(status: u16, content_type: &'static str, body: &str) -> Response {
    Response { status, content_type, body: body.as_bytes().to_vec(), set_cookie: None }
}

fn json_msg(status: u16, v: serde_json::Value) -> Response {
    Response { status, content_type: "application/json", body: v.to_string().into_bytes(), set_cookie: None }
}

fn login(shared: &Shared, req: &Request, now: f64) -> Response {
    if req.body.len() > MAX_BODY {
        return json_msg(413, json!({ "error": "requête trop grande" }));
    }
    let password = serde_json::from_slice::<serde_json::Value>(req.body)
        .ok()
        .and_then(|v| v["password"].as_str().map(String::from))
        .unwrap_or_default();
    match shared.auth.login(&password, now) {
        Login::Granted(token) => {
            let mut r = json_msg(200, json!({ "status": "ok" }));
            r.set_cookie = Some(auth::set_cookie(&token, req.https));
            r
        }
        Login::Denied => json_msg(401, json!({ "error": "mot de passe incorrect" })),
        Login::TooManyAttempts => json_msg(429, json!({ "error": "trop de tentatives, réessayer dans une minute" })),
    }
}

/// Route une requête : fichiers statiques et santé publics, tout le reste derrière la session.
pub fn route(shared: &Shared, req: &Request, now: f64) -> Response {
    let path = req.url.split('?').next().unwrap_or("");
    if req.method == "GET" {
        match path {
            "/" | "/index.html" => return plain(200, "text/html; charset=utf-8", INDEX_HTML),
            "/styles.css" => return plain(200, "text/css; charset=utf-8", STYLES_CSS),
            "/app.js" => return plain(200, "text/javascript; charset=utf-8", APP_JS),
            "/api/v1/health" => return api::handle("GET", req.url, &shared.history, &shared.info, now),
            _ => {}
        }
    }
    let token = req.cookie.and_then(auth::token_from_cookie).unwrap_or("");
    match (req.method, path) {
        ("POST", "/api/v1/login") => login(shared, req, now),
        ("POST", "/api/v1/logout") => {
            shared.auth.logout(token);
            let mut r = json_msg(200, json!({ "status": "ok" }));
            r.set_cookie = Some(format!("{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0", auth::COOKIE));
            r
        }
        _ if !path.starts_with("/api/") => json_msg(404, json!({ "error": "introuvable" })),
        _ if !shared.auth.is_valid(token, now) => json_msg(401, json!({ "error": "authentification requise" })),
        _ => api::handle(req.method, req.url, &shared.history, &shared.info, now),
    }
}

const WORKERS: usize = 4;
const POLL: Duration = Duration::from_millis(100);

fn now_s() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn request_header(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
}

fn serve_one(shared: &Shared, mut request: tiny_http::Request) {
    let (cookie, forwarded) = (request_header(&request, "Cookie"), request_header(&request, "X-Forwarded-Proto"));
    let mut body = Vec::new();
    let _ = request.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut body);
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let https = forwarded.is_some_and(|f| f.eq_ignore_ascii_case("https"));
    let r = route(shared, &Request { method: &method, url: &url, cookie: cookie.as_deref(), body: &body, https }, now_s());

    let mut resp = tiny_http::Response::from_data(r.body).with_status_code(r.status);
    let cache = if r.content_type.starts_with("application/") { "no-store" } else { "no-cache" };
    let mut headers = vec![
        header("Content-Type", r.content_type),
        header("Cache-Control", cache),
        header("X-Content-Type-Options", "nosniff"),
        header("Referrer-Policy", "no-referrer"),
        header("Content-Security-Policy", "default-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'"),
    ];
    if let Some(c) = &r.set_cookie {
        headers.push(header("Set-Cookie", c));
    }
    for h in headers.into_iter().flatten() {
        resp.add_header(h);
    }
    let _ = request.respond(resp);
}

/// Serveur HTTP en fonctionnement ; arrêté par `stop`.
pub struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl Running {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for w in self.workers {
            let _ = w.join();
        }
    }
}

fn bind_error(addr: SocketAddr, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("port {} déjà utilisé sur {} (un autre rpi-monitor ?) ; choisir un autre port avec --port", addr.port(), addr.ip())
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("port {} interdit sur {} : les ports < 1024 demandent des privilèges ; choisir un port plus élevé", addr.port(), addr.ip())
        }
        _ => format!("écoute impossible sur {addr} : {e}"),
    }
}

/// Démarre le serveur. `password` : `None` désactive l'authentification.
pub fn start(config: &Config, history: Arc<Mutex<History>>, password: Option<String>) -> Result<Running, String> {
    let listener = TcpListener::bind(config.addr).map_err(|e| bind_error(config.addr, &e))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let server = Arc::new(Server::from_listener(listener, None).map_err(|e| format!("serveur HTTP : {e}"))?);
    let shared = Arc::new(Shared {
        history,
        info: Info { demo: config.demo, started_at: now_s() },
        auth: Auth::new(password),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let workers = (0..WORKERS)
        .map(|i| {
            let (server, shared, stop) = (server.clone(), shared.clone(), stop.clone());
            std::thread::Builder::new()
                .name(format!("http-{i}"))
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        match server.recv_timeout(POLL) {
                            Ok(Some(request)) => serve_one(&shared, request),
                            Ok(None) => {}
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Running { addr, stop, workers })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(password: Option<&str>) -> Shared {
        Shared {
            history: Arc::new(Mutex::new(History::new())),
            info: Info { demo: true, started_at: 0.0 },
            auth: Auth::new(password.map(String::from)),
        }
    }

    fn req<'a>(method: &'a str, url: &'a str, cookie: Option<&'a str>, body: &'a [u8]) -> Request<'a> {
        Request { method, url, cookie, body, https: false }
    }

    fn cookie_of(r: &Response) -> String {
        let c = r.set_cookie.as_ref().expect("Set-Cookie");
        c.split(';').next().unwrap().to_string()
    }

    #[test]
    fn static_pages_and_health_are_public() {
        let s = shared(Some("pw"));
        for (url, ct) in [("/", "text/html"), ("/styles.css", "text/css"), ("/app.js", "text/javascript"), ("/api/v1/health", "application/json")] {
            let r = route(&s, &req("GET", url, None, b""), 1.0);
            assert_eq!(r.status, 200, "{url}");
            assert!(r.content_type.starts_with(ct), "{url}");
        }
    }

    #[test]
    fn data_requires_a_session() {
        let s = shared(Some("pw"));
        for url in ["/api/v1/metrics/current", "/api/v1/metrics/history", "/api/v1/metrics/stats", "/api/v1/metrics/events"] {
            assert_eq!(route(&s, &req("GET", url, None, b""), 1.0).status, 401, "{url}");
        }
        assert_eq!(route(&s, &req("GET", "/api/v1/metrics/current", Some("rpi_session=forged"), b""), 1.0).status, 401);
    }

    #[test]
    fn login_then_session_cookie_opens_the_api() {
        let s = shared(Some("pw"));
        let r = route(&s, &req("POST", "/api/v1/login", None, br#"{"password":"pw"}"#), 1.0);
        assert_eq!(r.status, 200);
        let c = cookie_of(&r);
        assert!(r.set_cookie.unwrap().contains("HttpOnly"));
        assert_eq!(route(&s, &req("GET", "/api/v1/metrics/current", Some(&c), b""), 2.0).status, 200);
    }

    #[test]
    fn wrong_or_malformed_login_is_401_without_cookie() {
        let s = shared(Some("pw"));
        for body in [&br#"{"password":"nope"}"#[..], b"not json", b"{}", b""] {
            let r = route(&s, &req("POST", "/api/v1/login", None, body), 1.0);
            assert_eq!(r.status, 401);
            assert!(r.set_cookie.is_none());
        }
    }

    #[test]
    fn repeated_failures_give_429() {
        let s = shared(Some("pw"));
        for _ in 0..5 {
            route(&s, &req("POST", "/api/v1/login", None, br#"{"password":"x"}"#), 1.0);
        }
        assert_eq!(route(&s, &req("POST", "/api/v1/login", None, br#"{"password":"pw"}"#), 2.0).status, 429);
    }

    #[test]
    fn oversized_login_body_is_refused() {
        let s = shared(Some("pw"));
        let big = vec![b'a'; MAX_BODY + 1];
        assert_eq!(route(&s, &req("POST", "/api/v1/login", None, &big), 1.0).status, 413);
    }

    #[test]
    fn logout_invalidates_the_session() {
        let s = shared(Some("pw"));
        let c = cookie_of(&route(&s, &req("POST", "/api/v1/login", None, br#"{"password":"pw"}"#), 1.0));
        let out = route(&s, &req("POST", "/api/v1/logout", Some(&c), b""), 2.0);
        assert!(out.set_cookie.unwrap().contains("Max-Age=0"));
        assert_eq!(route(&s, &req("GET", "/api/v1/metrics/current", Some(&c), b""), 3.0).status, 401);
    }

    #[test]
    fn secure_flag_only_behind_https() {
        let s = shared(Some("pw"));
        let r = Request { https: true, ..req("POST", "/api/v1/login", None, br#"{"password":"pw"}"#) };
        assert!(route(&s, &r, 1.0).set_cookie.unwrap().contains("Secure"));
        let r = route(&s, &req("POST", "/api/v1/login", None, br#"{"password":"pw"}"#), 1.0);
        assert!(!r.set_cookie.unwrap().contains("Secure"));
    }

    #[test]
    fn without_auth_the_api_is_open() {
        let s = shared(None);
        assert_eq!(route(&s, &req("GET", "/api/v1/metrics/current", None, b""), 1.0).status, 200);
    }

    #[test]
    fn writes_are_refused_and_unknown_paths_are_404() {
        let s = shared(None);
        assert_eq!(route(&s, &req("POST", "/api/v1/metrics/current", None, b""), 1.0).status, 405);
        assert_eq!(route(&s, &req("GET", "/nope", None, b""), 1.0).status, 404);
        assert_eq!(route(&s, &req("POST", "/", None, b""), 1.0).status, 404);
    }
}
