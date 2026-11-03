//! API JSON v1, en lecture seule. Fonction pure : testable sans socket.

use crate::archive::{round2, Point};
use crate::collector::{History, Key, ARCHIVE_S};
use serde_json::{json, Map, Value};
use std::sync::Mutex;

/// Version et commit du binaire en cours (affichés dans l'interface pour savoir ce qui tourne).
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " ", env!("RPI_MONITOR_BUILD"));

const STEP_S: f64 = 10.0;
const DEFAULT_PERIOD_S: f64 = 3600.0;
const DEFAULT_MAX_POINTS: usize = 600;

/// Contexte constant du service.
pub struct Info {
    pub demo: bool,
    pub started_at: f64,
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub set_cookie: Option<String>,
    pub script_nonce: Option<String>, // page d'accueil : nonce du script en ligne, repris dans la CSP
}

fn json_response(status: u16, v: Value) -> Response {
    Response { status, content_type: "application/json", body: v.to_string().into_bytes(), set_cookie: None, script_nonce: None }
}

fn error(status: u16, msg: impl Into<String>) -> Response {
    json_response(status, json!({ "error": msg.into() }))
}

/// (identifiant JSON, libellé, unité)
fn meta(key: Key) -> (&'static str, &'static str, &'static str) {
    match key {
        Key::Cpu => (key.id(), "CPU", "%"),
        Key::Freq => (key.id(), "Fréquence", "MHz"),
        Key::Power => (key.id(), "Consommation", "W"),
        Key::Ram => (key.id(), "RAM", "%"),
        Key::Temp => (key.id(), "Température", "°C"),
        Key::Fan => (key.id(), "Ventilateur", ""),
        Key::Throttle => (key.id(), "Throttling", ""),
    }
}

fn lock(h: &Mutex<History>) -> std::sync::MutexGuard<'_, History> {
    h.lock().unwrap_or_else(|e| e.into_inner())
}

fn split_url(url: &str) -> (&str, Vec<(&str, &str)>) {
    let (path, q) = url.split_once('?').unwrap_or((url, ""));
    (path, q.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).collect())
}

fn param<'a>(q: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

fn period(q: &[(&str, &str)]) -> Result<f64, String> {
    match param(q, "period") {
        None => Ok(DEFAULT_PERIOD_S),
        Some(v) => match v.parse::<f64>() {
            Ok(p) if p.is_finite() && p > 0.0 => Ok(p.min(ARCHIVE_S)),
            _ => Err(format!("period invalide : « {v} » ; attendu un nombre de secondes > 0")),
        },
    }
}

fn max_points(q: &[(&str, &str)]) -> Result<usize, String> {
    match param(q, "max_points") {
        None => Ok(DEFAULT_MAX_POINTS),
        Some(v) => match v.parse::<usize>() {
            Ok(n) if (10..=5000).contains(&n) => Ok(n),
            _ => Err(format!("max_points invalide : « {v} » ; attendu un entier de 10 à 5000")),
        },
    }
}

/// Regroupe les périodes de 10 s par pas multiples de 10 s (moyenne des moyennes, vrais min/max).
fn resample(points: &[Point], step: f64) -> Vec<Point> {
    let mut out: Vec<(Point, u32)> = vec![];
    for p in points {
        let ts = (p.ts / step).floor() * step;
        match out.last_mut() {
            Some((o, n)) if o.ts == ts => {
                o.mean += p.mean;
                o.min = o.min.min(p.min);
                o.max = o.max.max(p.max);
                *n += 1;
            }
            _ => out.push((Point { ts, ..*p }, 1)),
        }
    }
    out.into_iter().map(|(p, n)| Point { mean: p.mean / n as f64, ..p }).collect()
}

fn keys(q: &[(&str, &str)]) -> Result<Vec<Key>, String> {
    match param(q, "key") {
        None => Ok(Key::ALL.to_vec()),
        Some(name) => Key::ALL
            .into_iter()
            .find(|&k| meta(k).0 == name)
            .map(|k| vec![k])
            .ok_or_else(|| format!("key inconnue : « {name} »")),
    }
}

fn current(h: &History, info: &Info, now: f64) -> Value {
    let mut metrics = Map::new();
    for key in Key::ALL {
        let (id, label, unit) = meta(key);
        let mut m = json!({ "label": label, "unit": unit, "value": h.snapshot(key).last.map(round2) });
        match key {
            Key::Fan => {
                let f = h.fan_last();
                m["active"] = json!(f.and_then(|f| f.active()));
                m["state"] = json!(f.and_then(|f| f.state));
                m["pwm"] = json!(f.and_then(|f| f.pwm));
                m["rpm"] = json!(f.and_then(|f| f.rpm));
            }
            Key::Throttle => {
                let t = h.throttle_last();
                m["active"] = json!(t.as_ref().map(|t| t.active()));
                m["now"] = json!(t.as_ref().map(|t| t.now.clone()));
                m["since_boot"] = json!(t.as_ref().map(|t| t.since_boot.clone()));
            }
            Key::Cpu => m["cores"] = json!(h.cores_last().into_iter().map(|c| c.map(round2)).collect::<Vec<_>>()),
            _ => {}
        }
        metrics.insert(id.into(), m);
    }
    json!({ "ts": now, "demo": info.demo, "metrics": metrics })
}

fn history(h: &History, ks: &[Key], period_s: f64, max_points: usize, now: f64) -> Value {
    let step = ((period_s / max_points as f64 / STEP_S).ceil() * STEP_S).max(STEP_S);
    let mut series = Map::new();
    for &key in ks {
        let raw = h.archive_points(key, Some(now - period_s));
        let pts: Vec<Value> = resample(&raw, step).iter().map(|p| json!([p.ts, round2(p.mean), round2(p.min), round2(p.max)])).collect();
        series.insert(meta(key).0.into(), Value::Array(pts));
    }
    let mut out = json!({ "period_s": period_s, "step_s": step, "series": series });
    if ks.contains(&Key::Cpu) {
        let cores: Vec<Value> = h
            .archive_cores(Some(now - period_s))
            .iter()
            .map(|c| Value::Array(resample(c, step).iter().map(|p| json!([p.ts, round2(p.mean), round2(p.min), round2(p.max)])).collect()))
            .collect();
        out["cpu_cores"] = Value::Array(cores);
    }
    out
}

fn stats(h: &History, ks: &[Key], period_s: f64, now: f64) -> Value {
    let mut out = Map::new();
    for &key in ks {
        let s = h.archive_stats(key, Some(now - period_s));
        out.insert(meta(key).0.into(), json!(s.map(|s| json!({ "min": round2(s.minimum), "max": round2(s.maximum), "mean": round2(s.mean) }))));
    }
    json!({ "period_s": period_s, "stats": out })
}

/// Périodes actives consécutives fusionnées en intervalles.
fn intervals(points: &[Point]) -> Vec<Value> {
    let mut out: Vec<(f64, f64, f64)> = vec![]; // début, fin, pic
    for p in points.iter().filter(|p| p.mean > 0.0) {
        match out.last_mut() {
            Some(last) if (last.1 - p.ts).abs() < 1e-6 => {
                last.1 = p.ts + STEP_S;
                last.2 = last.2.max(p.mean);
            }
            _ => out.push((p.ts, p.ts + STEP_S, p.mean)),
        }
    }
    out.into_iter().map(|(start, end, peak)| json!({ "start": start, "end": end, "peak": round2(peak) })).collect()
}

fn events(h: &History, period_s: f64, now: f64) -> Value {
    let ev = |key| Value::Array(intervals(&h.archive_points(key, Some(now - period_s))));
    json!({ "period_s": period_s, "events": { "fan": ev(Key::Fan), "throttle": ev(Key::Throttle) } })
}

/// Traite une requête de l'API ; `now` en secondes epoch.
pub fn handle(method: &str, url: &str, history_lock: &Mutex<History>, info: &Info, now: f64) -> Response {
    if method != "GET" {
        return error(405, "lecture seule : seule la méthode GET est acceptée");
    }
    let (path, q) = split_url(url);
    let parsed = || -> Result<(Vec<Key>, f64), String> { Ok((keys(&q)?, period(&q)?)) };
    match path {
        "/api/v1/health" => {
            json_response(200, json!({ "status": "ok", "version": VERSION, "uptime_s": now - info.started_at, "demo": info.demo }))
        }
        "/api/v1/metrics/current" => json_response(200, current(&lock(history_lock), info, now)),
        "/api/v1/metrics/history" => match parsed().and_then(|(ks, p)| Ok((ks, p, max_points(&q)?))) {
            Ok((ks, p, m)) => json_response(200, history(&lock(history_lock), &ks, p, m, now)),
            Err(e) => error(400, e),
        },
        "/api/v1/metrics/stats" => match parsed() {
            Ok((ks, p)) => json_response(200, stats(&lock(history_lock), &ks, p, now)),
            Err(e) => error(400, e),
        },
        "/api/v1/metrics/events" => match period(&q) {
            Ok(p) => json_response(200, events(&lock(history_lock), p, now)),
            Err(e) => error(400, e),
        },
        _ => error(404, "route inconnue"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::Readings;
    use crate::sensors::{parse_throttled, FanReading};
    use serde_json::Value;

    fn info() -> Info {
        Info { demo: true, started_at: 1000.0 }
    }

    fn get(h: &Mutex<History>, url: &str, now: f64) -> (u16, Value) {
        let r = handle("GET", url, h, &info(), now);
        (r.status, serde_json::from_slice(&r.body).unwrap_or(Value::Null))
    }

    fn filled() -> Mutex<History> {
        let mut h = History::new();
        for t in 0..30 {
            h.record_slow(1000.0 + t as f64, &Readings { cpu: Some(10.0 + t as f64), temp: Some(40.0), ..Default::default() });
        }
        Mutex::new(h)
    }

    #[test]
    fn health_reports_uptime_and_mode() {
        let (s, v) = get(&Mutex::new(History::new()), "/api/v1/health", 1060.0);
        assert_eq!(s, 200);
        assert_eq!((v["status"].as_str(), v["uptime_s"].as_f64(), v["demo"].as_bool()), (Some("ok"), Some(60.0), Some(true)));
    }

    #[test]
    fn current_gives_label_unit_value_and_null_when_unavailable() {
        let (s, v) = get(&filled(), "/api/v1/metrics/current", 1030.0);
        assert_eq!(s, 200);
        assert_eq!(v["ts"].as_f64(), Some(1030.0));
        let cpu = &v["metrics"]["cpu"];
        assert_eq!((cpu["label"].as_str(), cpu["unit"].as_str(), cpu["value"].as_f64()), (Some("CPU"), Some("%"), Some(39.0)));
        assert!(v["metrics"]["ram"]["value"].is_null()); // jamais 0
        assert_eq!(v["metrics"].as_object().unwrap().len(), 7);
    }

    #[test]
    fn current_exposes_fan_and_throttle_details() {
        let mut h = History::new();
        h.record_fast(1000.0, Some(FanReading { state: Some(2), pwm: Some(0), rpm: None }));
        h.record_slow(1000.0, &Readings { throttle: Some(parse_throttled(0x50005)), ..Default::default() });
        let (_, v) = get(&Mutex::new(h), "/api/v1/metrics/current", 1001.0);
        let fan = &v["metrics"]["fan"];
        assert_eq!((fan["active"].as_bool(), fan["state"].as_i64(), fan["rpm"].is_null()), (Some(true), Some(2), true));
        let t = &v["metrics"]["throttle"];
        assert_eq!(t["active"].as_bool(), Some(true));
        assert_eq!(t["now"][0].as_str(), Some("sous-tension"));
        assert_eq!(t["since_boot"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn current_fan_is_null_when_unreadable() {
        let (_, v) = get(&Mutex::new(History::new()), "/api/v1/metrics/current", 1.0);
        assert!(v["metrics"]["fan"]["active"].is_null());
        assert!(v["metrics"]["throttle"]["active"].is_null());
    }

    #[test]
    fn history_returns_timestamped_points_filtered_by_period_and_key() {
        let (s, v) = get(&filled(), "/api/v1/metrics/history?period=20&key=cpu", 1030.0);
        assert_eq!(s, 200);
        assert_eq!(v["step_s"].as_f64(), Some(10.0));
        let cpu = v["series"]["cpu"].as_array().unwrap();
        assert_eq!(v["series"].as_object().unwrap().len(), 1);
        assert_eq!(cpu.last().unwrap()[0].as_f64(), Some(1020.0)); // [ts, moyenne, min, max]
        assert!(cpu.first().unwrap()[0].as_f64().unwrap() >= 1010.0);
    }

    #[test]
    fn history_defaults_to_every_key_and_one_hour() {
        let (_, v) = get(&filled(), "/api/v1/metrics/history", 1030.0);
        assert_eq!(v["period_s"].as_f64(), Some(3600.0));
        assert_eq!(v["series"].as_object().unwrap().len(), 7);
        assert!(v["series"]["ram"].as_array().unwrap().is_empty());
    }

    #[test]
    fn invalid_query_is_a_400_with_a_message() {
        for url in ["/api/v1/metrics/history?period=abc", "/api/v1/metrics/history?period=0", "/api/v1/metrics/history?key=nope", "/api/v1/metrics/stats?period=-5"] {
            let (s, v) = get(&filled(), url, 1030.0);
            assert_eq!(s, 400, "{url}");
            assert!(v["error"].as_str().is_some());
        }
    }

    #[test]
    fn period_is_capped_to_the_48h_retention() {
        let (s, v) = get(&filled(), "/api/v1/metrics/history?period=99999999", 1030.0);
        assert_eq!((s, v["period_s"].as_f64()), (200, Some(172800.0)));
    }

    #[test]
    fn stats_give_min_max_mean_or_null() {
        let (s, v) = get(&filled(), "/api/v1/metrics/stats?period=3600", 1030.0);
        assert_eq!(s, 200);
        let t = &v["stats"]["temp"];
        assert_eq!((t["min"].as_f64(), t["max"].as_f64(), t["mean"].as_f64()), (Some(40.0), Some(40.0), Some(40.0)));
        assert!(v["stats"]["ram"].is_null());
    }

    #[test]
    fn events_merge_consecutive_active_periods() {
        let mut h = History::new();
        let fan = |on| Some(FanReading { state: Some(on), pwm: None, rpm: None });
        for (t, on) in [(1000.0, 1), (1010.0, 1), (1020.0, 0), (1030.0, 1)] {
            h.record_fast(t, fan(on));
        }
        let (s, v) = get(&Mutex::new(h), "/api/v1/metrics/events?period=3600", 1040.0);
        assert_eq!(s, 200);
        let ev = v["events"]["fan"].as_array().unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0]["start"].as_f64(), ev[0]["end"].as_f64()), (Some(1000.0), Some(1020.0)));
        assert_eq!((ev[1]["start"].as_f64(), ev[1]["end"].as_f64()), (Some(1030.0), Some(1040.0)));
        assert!(v["events"]["throttle"].as_array().unwrap().is_empty());
    }

    #[test]
    fn unknown_route_is_404_and_writes_are_refused() {
        let h = filled();
        assert_eq!(get(&h, "/api/v1/nope", 1.0).0, 404);
        for m in ["POST", "PUT", "DELETE", "PATCH"] {
            assert_eq!(handle(m, "/api/v1/metrics/current", &h, &info(), 1.0).status, 405);
        }
    }

    #[test]
    fn concurrent_readers_do_not_block_each_other() {
        let h = std::sync::Arc::new(filled());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let h = h.clone();
                std::thread::spawn(move || (0..50).all(|_| handle("GET", "/api/v1/metrics/current", &h, &info(), 1030.0).status == 200))
            })
            .collect();
        assert!(threads.into_iter().all(|t| t.join().unwrap()));
    }

    fn long_history() -> Mutex<History> {
        let mut h = History::new();
        for t in (0..(24 * 3600)).step_by(10) {
            // température : 40 °C avec un pic isolé de 90 °C ; trou de 2 h sans mesure
            if (3600 * 10..3600 * 12).contains(&t) {
                continue;
            }
            let v = if t == 3600 * 5 { 90.0 } else { 40.0 };
            h.record_slow(t as f64, &Readings { temp: Some(v), ..Default::default() });
        }
        Mutex::new(h)
    }

    #[test]
    fn long_periods_are_resampled_but_keep_peaks() {
        let (s, v) = get(&long_history(), "/api/v1/metrics/history?key=temp&period=172800&max_points=300", 24.0 * 3600.0);
        assert_eq!(s, 200);
        let pts = v["series"]["temp"].as_array().unwrap();
        assert!(pts.len() <= 300, "{}", pts.len());
        let step = v["step_s"].as_f64().unwrap();
        assert!(step > 10.0 && step % 10.0 == 0.0);
        let max = pts.iter().map(|p| p[3].as_f64().unwrap()).fold(0.0, f64::max);
        assert_eq!(max, 90.0, "le pic doit survivre au rééchantillonnage");
    }

    #[test]
    fn resampling_does_not_fill_gaps_with_zeros() {
        let (_, v) = get(&long_history(), "/api/v1/metrics/history?key=temp&period=172800&max_points=300", 24.0 * 3600.0);
        let step = v["step_s"].as_f64().unwrap();
        let pts = v["series"]["temp"].as_array().unwrap();
        assert!(pts.iter().all(|p| p[1].as_f64().unwrap() > 0.0));
        let biggest_gap = pts.windows(2).map(|w| w[1][0].as_f64().unwrap() - w[0][0].as_f64().unwrap()).fold(0.0, f64::max);
        assert!(biggest_gap > 3600.0 * 1.5, "le trou doit rester visible ({biggest_gap}, pas {step})");
    }

    #[test]
    fn short_periods_keep_the_native_10_second_resolution() {
        let (_, v) = get(&long_history(), "/api/v1/metrics/history?key=temp&period=900", 24.0 * 3600.0);
        assert_eq!(v["step_s"].as_f64(), Some(10.0));
        assert!(v["series"]["temp"].as_array().unwrap().len() >= 89);
    }

    #[test]
    fn max_points_is_validated() {
        for bad in ["abc", "0", "5", "999999"] {
            let (s, _) = get(&filled(), &format!("/api/v1/metrics/history?max_points={bad}"), 1030.0);
            assert_eq!(s, 400, "{bad}");
        }
    }

    #[test]
    fn default_history_is_light_enough_for_polling() {
        let (_, v) = get(&long_history(), "/api/v1/metrics/history?period=172800", 24.0 * 3600.0);
        let total: usize = v["series"].as_object().unwrap().values().map(|a| a.as_array().unwrap().len()).sum();
        assert!(total <= 7 * 600);
    }

    #[test]
    fn numbers_are_rounded_to_two_decimals_to_keep_payloads_small() {
        let mut h = History::new();
        h.record_slow(1000.0, &Readings { cpu: Some(12.345678), temp: Some(40.004), ..Default::default() });
        let h = Mutex::new(h);
        let (_, cur) = get(&h, "/api/v1/metrics/current", 1001.0);
        assert_eq!(cur["metrics"]["cpu"]["value"].as_f64(), Some(12.35));
        let (_, hist) = get(&h, "/api/v1/metrics/history?key=cpu", 1001.0);
        assert_eq!(hist["series"]["cpu"][0][1].as_f64(), Some(12.35));
        let (_, st) = get(&h, "/api/v1/metrics/stats", 1001.0);
        assert_eq!(st["stats"]["temp"]["mean"].as_f64(), Some(40.0));
    }

    #[test]
    fn health_reports_the_running_build() {
        let (_, v) = get(&Mutex::new(History::new()), "/api/v1/health", 1.0);
        let version = v["version"].as_str().unwrap();
        assert!(version.starts_with(env!("CARGO_PKG_VERSION")) && version.len() > env!("CARGO_PKG_VERSION").len(), "{version}");
        assert_eq!(version, VERSION);
    }

    fn with_cores() -> Mutex<History> {
        let mut h = History::new();
        for t in 0..30 {
            h.record_slow(1000.0 + t as f64, &Readings { cpu: Some(40.0), cores: vec![Some(10.0), Some(90.0 + (t % 2) as f64 / 3.0), None, Some(40.0)], ..Default::default() });
        }
        Mutex::new(h)
    }

    #[test]
    fn current_cpu_lists_each_core_with_null_when_unavailable() {
        let (_, v) = get(&with_cores(), "/api/v1/metrics/current", 1030.0);
        let cores = v["metrics"]["cpu"]["cores"].as_array().unwrap();
        assert_eq!(cores.len(), 4);
        assert_eq!((cores[0].as_f64(), cores[3].as_f64()), (Some(10.0), Some(40.0)));
        assert!(cores[2].is_null(), "un cœur sans mesure : null, jamais 0");
        assert_eq!(v["metrics"]["cpu"]["value"].as_f64(), Some(40.0), "le chiffre reste la moyenne globale");
    }

    #[test]
    fn current_cpu_cores_is_empty_when_the_machine_gives_none() {
        let (_, v) = get(&filled(), "/api/v1/metrics/current", 1030.0);
        assert_eq!(v["metrics"]["cpu"]["cores"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn history_gives_one_series_per_core_next_to_the_cpu_series() {
        let (s, v) = get(&with_cores(), "/api/v1/metrics/history?key=cpu&period=60", 1030.0);
        assert_eq!(s, 200);
        let cores = v["cpu_cores"].as_array().unwrap();
        assert_eq!(cores.len(), 4);
        assert_eq!(cores[0][0][1].as_f64(), Some(10.0)); // [ts, moyenne, min, max]
        assert!(cores[2].as_array().unwrap().is_empty());
        assert_eq!(v["series"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn core_series_only_come_with_the_cpu() {
        let (_, v) = get(&with_cores(), "/api/v1/metrics/history?key=temp", 1030.0);
        assert!(v.get("cpu_cores").is_none());
        let (_, all) = get(&with_cores(), "/api/v1/metrics/history", 1030.0);
        assert_eq!(all["cpu_cores"].as_array().unwrap().len(), 4);
    }
}
