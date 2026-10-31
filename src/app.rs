//! État de l'application et mise en forme des indicateurs (pur : sans terminal).

use crate::collector::{History, Key};
use crate::sensors::Throttle;
use ratatui::style::Color;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub key: Key,
    pub title: &'static str,
    pub unit: &'static str,
    pub color: Color,
    pub decimals: usize,
    pub fixed: Option<(f64, f64)>, // échelle fixe, sinon auto
    pub min_span: f64,             // amplitude minimale en échelle auto
    pub bars: bool,                // histogramme (ventilateur) plutôt que courbe
    pub step_s: f64,               // pas d'échantillonnage des valeurs stockées
    pub compact: bool,             // sans gros chiffre : ligne plus basse
}

const fn spec(key: Key, title: &'static str, unit: &'static str, color: Color) -> Spec {
    Spec { key, title, unit, color, decimals: 1, fixed: None, min_span: 10.0, bars: false, step_s: 1.0, compact: false }
}

pub const SPECS: [Spec; 7] = [
    Spec { decimals: 0, fixed: Some((0.0, 100.0)), ..spec(Key::Cpu, "CPU", "%", Color::Green) },
    Spec { decimals: 0, min_span: 400.0, ..spec(Key::Freq, "Fréquence", "MHz", Color::Cyan) },
    Spec { decimals: 2, min_span: 2.0, ..spec(Key::Power, "Consommation", "W", Color::LightYellow) },
    Spec { decimals: 1, fixed: Some((0.0, 100.0)), ..spec(Key::Ram, "RAM", "%", Color::Magenta) },
    Spec { decimals: 1, min_span: 10.0, ..spec(Key::Temp, "Température", "°C", Color::Red) },
    Spec { bars: true, fixed: Some((0.0, 1.0)), step_s: 10.0, ..spec(Key::Fan, "Ventilateur", "tr/min", Color::Yellow) },
    Spec {
        bars: true,
        fixed: Some((0.0, 1.0)),
        step_s: 10.0,
        compact: true,
        ..spec(Key::Throttle, "Throttling", "flags", Color::LightRed)
    },
];

/// Durées d'affichage : 1, 5 puis 30 minutes.
pub const WINDOWS: [f64; 3] = [60.0, 300.0, 1800.0];
const INITIAL_WINDOW: usize = 1; // 5 min

#[derive(Debug, Clone, PartialEq)]
pub struct LaneView {
    pub big: String,
    pub stats: String,
    pub values: Vec<f64>,
    pub window_s: f64,
}

fn fmt(v: Option<f64>, decimals: usize) -> String {
    v.map_or_else(|| "--".to_string(), |v| format!("{v:.decimals$}"))
}

fn throttle_summary(thr: Option<&Throttle>) -> String {
    match thr {
        None => "N/A".into(),
        Some(t) if !t.now.is_empty() => t.now.join("+"),
        Some(t) if !t.since_boot.is_empty() => format!("OK (déjà: {})", t.since_boot.join("+")),
        Some(_) => "OK".into(),
    }
}

pub fn lane_view(spec: &Spec, history: &History, window_s: f64) -> LaneView {
    let snap = history.snapshot(spec.key);
    let (big, stats) = match spec.key {
        Key::Throttle => {
            let thr = history.throttle_last();
            let big = thr.as_ref().map_or_else(|| "--".to_string(), |t| t.now.len().to_string());
            (big, throttle_summary(thr.as_ref()))
        }
        Key::Fan => {
            let fan = history.fan_last();
            let state = match fan.and_then(|f| f.active()) {
                Some(true) => "ON",
                Some(false) => "OFF",
                None => "N/A",
            };
            let mut stats = format!(
                "{state} état {} pwm {}",
                fmt(fan.and_then(|f| f.state).map(|v| v as f64), 0),
                fmt(fan.and_then(|f| f.pwm).map(|v| v as f64), 0)
            );
            if let Some(st) = snap.stats {
                stats.push_str(&format!(" · actif {:.0}%", st.mean * 100.0));
            }
            (fmt(fan.and_then(|f| f.rpm).map(|v| v as f64), 0), stats)
        }
        _ => {
            let d = spec.decimals;
            let stats = snap.stats.map_or_else(
                || "N/A".to_string(),
                |s| format!("min {} max {} moy {}", fmt(Some(s.minimum), d), fmt(Some(s.maximum), d), fmt(Some(s.mean), d)),
            );
            (fmt(snap.last, d), stats)
        }
    };
    LaneView { big, stats, values: snap.values, window_s }
}

pub struct App {
    history: Arc<Mutex<History>>,
    demo: bool,
    paused: bool,
    window_idx: usize,
    quit: bool,
    views: Vec<LaneView>,
}

impl App {
    pub fn new(history: Arc<Mutex<History>>, demo: bool) -> Self {
        let mut app = Self { history, demo, paused: false, window_idx: INITIAL_WINDOW, quit: false, views: vec![] };
        app.rebuild();
        app
    }

    /// Touches : q quitter · p pause de l'affichage · r reset historique · w fenêtre.
    pub fn on_key(&mut self, c: char) {
        match c {
            'q' => self.quit = true,
            'p' => {
                self.paused = !self.paused;
                self.refresh();
            }
            'r' => {
                self.history.lock().unwrap_or_else(|e| e.into_inner()).reset();
                self.refresh();
            }
            'w' => {
                self.window_idx = (self.window_idx + 1) % WINDOWS.len();
                self.rebuild(); // appliqué même en pause
            }
            _ => {}
        }
    }

    /// Rafraîchit l'affichage ; sans effet en pause (la collecte, elle, continue).
    pub fn refresh(&mut self) {
        if !self.paused {
            self.rebuild();
        }
    }

    fn rebuild(&mut self) {
        let w = self.window_s();
        let h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        self.views = SPECS.iter().map(|s| lane_view(s, &h, w)).collect();
    }

    pub fn views(&self) -> &[LaneView] {
        &self.views
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn window_s(&self) -> f64 {
        WINDOWS[self.window_idx]
    }

    pub fn subtitle(&self) -> String {
        let mut parts = vec![];
        if self.demo {
            parts.push("DEMO".to_string());
        }
        parts.push(format!("fenêtre {} min", self.window_s() as u32 / 60));
        if self.paused {
            parts.push("PAUSE (collecte active)".to_string());
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::Readings;
    use crate::sensors::{parse_throttled, FanReading};

    fn app() -> (App, Arc<Mutex<History>>) {
        let h = Arc::new(Mutex::new(History::new()));
        (App::new(h.clone(), false), h)
    }

    fn cpu(h: &Arc<Mutex<History>>, ts: f64, v: f64) {
        h.lock().unwrap().record_slow(ts, &Readings { cpu: Some(v), ..Default::default() });
    }

    #[test]
    fn indicators_display_order() {
        let keys: Vec<Key> = SPECS.iter().map(|s| s.key).collect();
        assert_eq!(
            keys,
            [Key::Cpu, Key::Freq, Key::Power, Key::Ram, Key::Temp, Key::Fan, Key::Throttle]
        );
        assert!(SPECS[6].compact && SPECS[5].bars && SPECS[6].bars);
    }

    #[test]
    fn initial_window_is_five_minutes_and_cycles_1_5_30() {
        let (mut a, _) = app();
        assert_eq!(a.window_s(), 300.0);
        a.on_key('w');
        assert_eq!(a.window_s(), 1800.0);
        a.on_key('w');
        assert_eq!(a.window_s(), 60.0);
        a.on_key('w');
        assert_eq!(a.window_s(), 300.0);
    }

    #[test]
    fn subtitle_shows_demo_window_and_pause() {
        let h = Arc::new(Mutex::new(History::new()));
        let mut a = App::new(h, true);
        assert_eq!(a.subtitle(), "DEMO · fenêtre 5 min");
        a.on_key('p');
        assert_eq!(a.subtitle(), "DEMO · fenêtre 5 min · PAUSE (collecte active)");
        let (a2, _) = app();
        assert_eq!(a2.subtitle(), "fenêtre 5 min");
    }

    #[test]
    fn q_quits_other_keys_do_not() {
        let (mut a, _) = app();
        a.on_key('x');
        assert!(!a.should_quit());
        a.on_key('q');
        assert!(a.should_quit());
    }

    #[test]
    fn pause_freezes_display_but_history_keeps_growing() {
        let (mut a, h) = app();
        cpu(&h, 100.0, 10.0);
        a.refresh();
        assert_eq!(a.views()[0].big, "10");
        a.on_key('p');
        assert!(a.paused());
        cpu(&h, 101.0, 90.0);
        a.refresh();
        assert_eq!(a.views()[0].big, "10"); // affichage figé
        assert_eq!(h.lock().unwrap().snapshot(Key::Cpu).values.len(), 2); // collecte continue
        a.on_key('p');
        a.refresh();
        assert_eq!(a.views()[0].big, "90");
    }

    #[test]
    fn reset_clears_history() {
        let (mut a, h) = app();
        cpu(&h, 100.0, 10.0);
        a.refresh();
        a.on_key('r');
        assert!(h.lock().unwrap().snapshot(Key::Cpu).values.is_empty());
        assert_eq!(a.views()[0].big, "--");
    }

    #[test]
    fn window_change_applies_even_while_paused_and_stays_paused() {
        let (mut a, _) = app();
        a.refresh();
        a.on_key('p');
        a.on_key('w');
        assert!(a.paused());
        assert_eq!(a.views()[0].window_s, 1800.0);
    }

    #[test]
    fn numeric_lane_shows_value_and_min_max_mean() {
        let mut h = History::new();
        for (i, v) in [10.0, 30.0, 20.0].into_iter().enumerate() {
            h.record_slow(100.0 + i as f64, &Readings { cpu: Some(v), ..Default::default() });
        }
        let v = lane_view(&SPECS[0], &h, 300.0);
        assert_eq!((v.big.as_str(), v.stats.as_str()), ("20", "min 10 max 30 moy 20"));
        let p = lane_view(&SPECS[2], &h, 300.0);
        assert_eq!((p.big.as_str(), p.stats.as_str()), ("--", "N/A"));
    }

    #[test]
    fn fan_lane_shows_rpm_state_pwm_and_active_share() {
        let mut h = History::new();
        h.record_fast(100.0, Some(FanReading { state: Some(1), pwm: Some(75), rpm: Some(1700) }));
        let v = lane_view(&SPECS[5], &h, 300.0);
        assert_eq!(v.big, "1700");
        assert_eq!(v.stats, "ON état 1 pwm 75 · actif 100%");
        let none = lane_view(&SPECS[5], &History::new(), 300.0);
        assert_eq!((none.big.as_str(), none.stats.as_str()), ("--", "N/A état -- pwm --"));
    }

    #[test]
    fn throttle_lane_distinguishes_now_from_since_boot() {
        let at = |raw| {
            let mut h = History::new();
            h.record_slow(100.0, &Readings { throttle: Some(parse_throttled(raw)), ..Default::default() });
            lane_view(&SPECS[6], &h, 300.0)
        };
        assert_eq!((at(0).big.as_str(), at(0).stats.as_str()), ("0", "OK"));
        assert_eq!(at(0x80000).stats, "OK (déjà: limite temp)");
        let now = at(0x5);
        assert_eq!((now.big.as_str(), now.stats.as_str()), ("2", "sous-tension+throttle"));
        let unknown = lane_view(&SPECS[6], &History::new(), 300.0);
        assert_eq!((unknown.big.as_str(), unknown.stats.as_str()), ("--", "N/A"));
    }
}
