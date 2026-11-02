//! Thread de collecte + historiques. Indépendant de l'affichage.
//! (Les capteurs eux-mêmes sont dans `sensors/` ; ici : planification et stockage.)

use crate::archive::{Point, Series};
use crate::history::{BucketSeries, RingBuffer, Stats};
use crate::sensors::{parse_throttled, FanReading, Sensors, Throttle};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const HISTORY_S: usize = 30 * 60;
const BUCKET_S: f64 = 10.0;
const FAST: Duration = Duration::from_millis(500); // ventilateur
const SLOW_EVERY: u32 = 2; // autres métriques : 1 s
pub const ARCHIVE_S: f64 = 48.0 * 3600.0; // historique long
const ARCHIVE_STEP_S: f64 = 10.0;
pub const MAX_CORES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Cpu,
    Freq,
    Power,
    Ram,
    Temp,
    Fan,
    Throttle,
}

impl Key {
    pub const ALL: [Key; 7] = [Key::Cpu, Key::Freq, Key::Power, Key::Ram, Key::Temp, Key::Fan, Key::Throttle];

    /// Identifiant stable (API, fichier de sauvegarde).
    pub fn id(self) -> &'static str {
        match self {
            Key::Cpu => "cpu",
            Key::Freq => "freq",
            Key::Power => "power",
            Key::Ram => "ram",
            Key::Temp => "temp",
            Key::Fan => "fan",
            Key::Throttle => "throttle",
        }
    }

    fn index(self) -> usize {
        Key::ALL.iter().position(|&k| k == self).unwrap_or(0)
    }
}

/// Copie des données d'un indicateur, prise sous verrou.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub values: Vec<f64>,
    pub last: Option<f64>,
    pub stats: Option<Stats>,
}

/// Mesures lentes (1 s) d'un relevé.
#[derive(Debug, Clone, Default)]
pub struct Readings {
    pub cpu: Option<f64>,
    pub freq: Option<f64>,
    pub power: Option<f64>,
    pub ram: Option<f64>,
    pub temp: Option<f64>,
    pub throttle: Option<Throttle>,
    pub cores: Vec<Option<f64>>, // charge de chaque cœur ; vide = indisponible
}

/// Historiques de tous les indicateurs (30 min) + dernières lectures détaillées.
pub struct History {
    rings: [RingBuffer; 5], // cpu, freq, power, ram, temp
    fan: BucketSeries,
    throttle: BucketSeries,
    fan_last: Option<FanReading>,
    throttle_last: Option<Throttle>,
    archive: [Series; 7], // 48 h horodatées, indexées comme `Key::ALL`
    cores: Vec<Series>,   // 48 h par cœur de CPU, créées au fur et à mesure
}

fn ring_index(key: Key) -> Option<usize> {
    match key {
        Key::Cpu => Some(0),
        Key::Freq => Some(1),
        Key::Power => Some(2),
        Key::Ram => Some(3),
        Key::Temp => Some(4),
        Key::Fan | Key::Throttle => None,
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            rings: std::array::from_fn(|_| RingBuffer::new(HISTORY_S)),
            fan: BucketSeries::new(BUCKET_S, HISTORY_S / BUCKET_S as usize),
            throttle: BucketSeries::new(BUCKET_S, HISTORY_S / BUCKET_S as usize),
            fan_last: None,
            throttle_last: None,
            archive: std::array::from_fn(|_| Series::new(ARCHIVE_STEP_S, ARCHIVE_S)),
            cores: vec![],
        }
    }

    pub fn record_fast(&mut self, ts: f64, fan: Option<FanReading>) {
        let active = fan.and_then(|f| f.active());
        self.fan.add(ts, active);
        self.archive[Key::Fan.index()].add(ts, active.map(f64::from));
        self.fan_last = fan;
    }

    pub fn record_slow(&mut self, ts: f64, r: &Readings) {
        for (ring, v) in self.rings.iter_mut().zip([r.cpu, r.freq, r.power, r.ram, r.temp]) {
            ring.add(v);
        }
        for (key, v) in [Key::Cpu, Key::Freq, Key::Power, Key::Ram, Key::Temp].into_iter().zip([r.cpu, r.freq, r.power, r.ram, r.temp]) {
            self.archive[key.index()].add(ts, v);
        }
        for (i, v) in r.cores.iter().take(MAX_CORES).enumerate() {
            if self.cores.len() <= i {
                self.cores.push(Series::new(ARCHIVE_STEP_S, ARCHIVE_S));
            }
            self.cores[i].add(ts, *v);
        }
        let throttling = r.throttle.as_ref().map(Throttle::active);
        self.throttle.add(ts, throttling);
        self.archive[Key::Throttle.index()].add(ts, throttling.map(f64::from));
        self.throttle_last = r.throttle.clone();
    }

    pub fn snapshot(&self, key: Key) -> Snapshot {
        match (ring_index(key), key) {
            (Some(i), _) => {
                let r = &self.rings[i];
                Snapshot { values: r.values(), last: r.last(), stats: r.stats() }
            }
            (None, Key::Fan) => bucket_snapshot(&self.fan),
            _ => bucket_snapshot(&self.throttle),
        }
    }

    /// Historique long (48 h) d'un indicateur, à partir de `since` (s, epoch).
    pub fn archive_points(&self, key: Key, since: Option<f64>) -> Vec<Point> {
        self.archive[key.index()].points(since)
    }

    /// Périodes terminées de chaque indicateur, pour la sauvegarde.
    pub fn archive_export(&self) -> Vec<(Key, Vec<Point>)> {
        Key::ALL.into_iter().map(|k| (k, self.archive[k.index()].completed())).collect()
    }

    pub fn archive_import(&mut self, data: Vec<(Key, Vec<Point>)>) {
        for (key, points) in data {
            self.archive[key.index()].restore(points);
        }
    }

    /// Historique long de chaque cœur de CPU (une série par cœur, dans l'ordre).
    pub fn archive_cores(&self, since: Option<f64>) -> Vec<Vec<Point>> {
        self.cores.iter().map(|c| c.points(since)).collect()
    }

    pub fn cores_export(&self) -> Vec<Vec<Point>> {
        self.cores.iter().map(Series::completed).collect()
    }

    pub fn cores_import(&mut self, data: Vec<Vec<Point>>) {
        self.cores = data
            .into_iter()
            .take(MAX_CORES)
            .map(|points| {
                let mut s = Series::new(ARCHIVE_STEP_S, ARCHIVE_S);
                s.restore(points);
                s
            })
            .collect();
    }

    pub fn archive_stats(&self, key: Key, since: Option<f64>) -> Option<Stats> {
        self.archive[key.index()].stats(since)
    }

    pub fn fan_last(&self) -> Option<FanReading> {
        self.fan_last
    }

    pub fn throttle_last(&self) -> Option<Throttle> {
        self.throttle_last.clone()
    }

    pub fn reset(&mut self) {
        self.rings.iter_mut().for_each(RingBuffer::clear);
        self.fan.clear();
        self.throttle.clear();
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

fn bucket_snapshot(b: &BucketSeries) -> Snapshot {
    Snapshot { values: b.values(), last: b.last(), stats: b.stats() }
}

/// Un capteur en échec ou qui panique donne `None` : la collecte ne s'arrête jamais.
fn safe<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok().flatten()
}

fn now_s() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn run(mut sensors: Box<dyn Sensors>, history: Arc<Mutex<History>>, stop: Receiver<()>) {
    let start = Instant::now();
    let mut tick: u32 = 0;
    loop {
        let ts = now_s();
        let fan = safe(|| sensors.fan());
        lock(&history).record_fast(ts, fan);
        if tick.is_multiple_of(SLOW_EVERY) {
            let r = Readings {
                cpu: safe(|| sensors.cpu_percent()),
                freq: safe(|| sensors.cpu_freq_mhz()),
                power: safe(|| sensors.power_w()),
                ram: safe(|| sensors.ram_percent()),
                temp: safe(|| sensors.temperature_c()),
                throttle: safe(|| sensors.throttled()).map(parse_throttled),
                cores: catch_unwind(AssertUnwindSafe(|| sensors.cpu_cores_percent())).unwrap_or_default(),
            };
            lock(&history).record_slow(ts, &r);
        }
        tick += 1;
        // horloge monotone : pas de dérive cumulée
        let wait = (start + FAST * tick).saturating_duration_since(Instant::now());
        if !matches!(stop.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
            break;
        }
    }
}

fn lock(h: &Mutex<History>) -> MutexGuard<'_, History> {
    h.lock().unwrap_or_else(|e| e.into_inner())
}

/// Thread de collecte : ventilateur toutes les 500 ms, le reste toutes les secondes.
pub struct Monitor {
    history: Arc<Mutex<History>>,
    stop_tx: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn start(sensors: Box<dyn Sensors>) -> Self {
        Self::start_with(sensors, History::new())
    }

    /// Démarre la collecte sur un historique existant (ex. restauré d'une sauvegarde).
    pub fn start_with(sensors: Box<dyn Sensors>, history: History) -> Self {
        let history = Arc::new(Mutex::new(history));
        let (stop_tx, stop_rx) = mpsc::channel();
        let h = history.clone();
        let handle = std::thread::Builder::new()
            .name("collector".into())
            .spawn(move || run(sensors, h, stop_rx))
            .expect("démarrage du thread de collecte");
        Self { history, stop_tx: Some(stop_tx), handle: Some(handle) }
    }

    pub fn history(&self) -> Arc<Mutex<History>> {
        self.history.clone()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop_tx.take(); // l'émetteur détruit réveille le thread
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn slow(cpu: Option<f64>, throttle: Option<Throttle>) -> Readings {
        Readings { cpu, throttle, ..Default::default() }
    }

    #[test]
    fn slow_readings_are_stored_per_indicator() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cpu: Some(12.0), temp: Some(40.0), ..Default::default() });
        assert_eq!(h.snapshot(Key::Cpu).last, Some(12.0));
        assert_eq!(h.snapshot(Key::Temp).last, Some(40.0));
        assert_eq!(h.snapshot(Key::Ram).last, None);
    }

    #[test]
    fn unavailable_reading_is_nan_in_series_not_zero() {
        let mut h = History::new();
        h.record_slow(100.0, &slow(Some(10.0), None));
        h.record_slow(101.0, &slow(None, None));
        let s = h.snapshot(Key::Cpu);
        assert_eq!(s.last, None);
        assert_eq!(s.values[0], 10.0);
        assert!(s.values[1].is_nan());
        assert_eq!(s.stats.unwrap().mean, 10.0);
    }

    #[test]
    fn fan_activation_on_a_single_measure_stays_visible_in_its_bucket() {
        let mut h = History::new();
        let fan = |on| Some(FanReading { state: Some(on), pwm: Some(0), rpm: Some(0) });
        for i in 0..20 {
            h.record_fast(100.0 + i as f64 * 0.5, fan(i64::from(i == 7)));
        }
        let s = h.snapshot(Key::Fan);
        assert_eq!(s.values.len(), 1);
        assert!(s.values[0] > 0.0);
        assert_eq!(h.fan_last().unwrap().state, Some(0));
    }

    #[test]
    fn throttle_activity_is_bucketed_and_distinguishes_now_from_since_boot() {
        let mut h = History::new();
        let past_only = crate::sensors::parse_throttled(0x80000);
        h.record_slow(100.0, &slow(None, Some(past_only.clone())));
        assert_eq!(h.snapshot(Key::Throttle).values, vec![0.0]); // survenu avant : pas en cours
        assert_eq!(h.throttle_last().unwrap().since_boot, ["limite temp"]);
        h.record_slow(101.0, &slow(None, Some(crate::sensors::parse_throttled(0x4))));
        assert!(h.snapshot(Key::Throttle).values[0] > 0.0);
    }

    #[test]
    fn reset_clears_every_indicator() {
        let mut h = History::new();
        h.record_slow(100.0, &slow(Some(1.0), Some(crate::sensors::parse_throttled(0))));
        h.record_fast(100.0, Some(FanReading { state: Some(1), pwm: None, rpm: None }));
        h.reset();
        for k in [Key::Cpu, Key::Fan, Key::Throttle] {
            assert!(h.snapshot(k).values.is_empty());
        }
    }


    #[test]
    fn archive_receives_slow_and_fast_readings_with_timestamps() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cpu: Some(12.0), ..Default::default() });
        let p = h.archive_points(Key::Cpu, None);
        assert_eq!((p[0].ts, p[0].mean), (100.0, 12.0));
        assert!(h.archive_points(Key::Ram, None).is_empty()); // indisponible : pas de point
        let fan = |on| Some(FanReading { state: Some(on), pwm: None, rpm: None });
        h.record_fast(100.0, fan(1));
        h.record_fast(100.5, fan(0));
        assert_eq!(h.archive_points(Key::Fan, None)[0].mean, 0.5);
        h.record_slow(101.0, &slow(None, Some(crate::sensors::parse_throttled(0x4))));
        assert_eq!(h.archive_points(Key::Throttle, None)[0].mean, 1.0);
    }

    #[test]
    fn reset_does_not_touch_the_archive() {
        let mut h = History::new();
        h.record_slow(100.0, &slow(Some(1.0), None));
        h.reset();
        assert_eq!(h.archive_points(Key::Cpu, None).len(), 1);
        assert!(h.snapshot(Key::Cpu).values.is_empty());
    }

    #[test]
    fn archive_keeps_48_hours() {
        let mut h = History::new();
        h.record_slow(0.0, &slow(Some(1.0), None));
        h.record_slow(49.0 * 3600.0, &slow(Some(2.0), None));
        let p = h.archive_points(Key::Cpu, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].mean, 2.0);
    }

    struct Fake;
    impl Sensors for Fake {
        fn cpu_percent(&mut self) -> Option<f64> { Some(1.0) }
        fn cpu_freq_mhz(&mut self) -> Option<f64> { Some(1500.0) }
        fn ram_percent(&mut self) -> Option<f64> { Some(10.0) }
        fn temperature_c(&mut self) -> Option<f64> { Some(40.0) }
        fn power_w(&mut self) -> Option<f64> { Some(3.5) }
        fn fan(&mut self) -> Option<FanReading> { None }
    }

    #[test]
    fn monitor_records_in_background() {
        let m = Monitor::start(Box::new(Fake));
        std::thread::sleep(Duration::from_millis(700));
        let h = m.history();
        assert_eq!(h.lock().unwrap().snapshot(Key::Power).last, Some(3.5));
        assert_eq!(h.lock().unwrap().snapshot(Key::Temp).last, Some(40.0));
        m.stop();
    }

    struct Broken;
    impl Sensors for Broken {
        fn cpu_percent(&mut self) -> Option<f64> { panic!("capteur en panne") }
        fn cpu_freq_mhz(&mut self) -> Option<f64> { None }
        fn ram_percent(&mut self) -> Option<f64> { Some(10.0) }
        fn temperature_c(&mut self) -> Option<f64> { Some(40.0) }
        fn fan(&mut self) -> Option<FanReading> { panic!("capteur en panne") }
    }

    #[test]
    fn monitor_survives_failing_and_panicking_sensors() {
        let m = Monitor::start(Box::new(Broken));
        std::thread::sleep(Duration::from_millis(1200));
        let h = m.history();
        assert_eq!(h.lock().unwrap().snapshot(Key::Temp).last, Some(40.0));
        assert_eq!(h.lock().unwrap().snapshot(Key::Cpu).last, None);
        assert!(h.lock().unwrap().snapshot(Key::Temp).values.len() >= 2); // la collecte continue
        m.stop();
    }

    #[test]
    fn stop_is_prompt_and_halts_collection() {
        let m = Monitor::start(Box::new(Fake));
        let h = m.history();
        let t = Instant::now();
        m.stop();
        assert!(t.elapsed() < Duration::from_millis(400), "arrêt trop lent");
        let n = h.lock().unwrap().snapshot(Key::Temp).values.len();
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(h.lock().unwrap().snapshot(Key::Temp).values.len(), n);
    }

    #[test]
    fn per_core_load_is_archived_one_series_per_core() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cores: vec![Some(10.0), Some(90.0), None, Some(40.0)], ..Default::default() });
        h.record_slow(101.0, &Readings { cores: vec![Some(20.0), Some(80.0), None, Some(60.0)], ..Default::default() });
        let cores = h.archive_cores(None);
        assert_eq!(cores.len(), 4);
        assert_eq!(cores[0][0].mean, 15.0);
        assert_eq!(cores[1][0].max, 90.0);
        assert!(cores[2].is_empty(), "un cœur sans mesure n'a pas de point, jamais 0");
        assert_eq!(cores[3][0].ts, 100.0);
    }

    #[test]
    fn readings_without_cores_leave_the_core_archive_empty() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cpu: Some(5.0), ..Default::default() });
        assert!(h.archive_cores(None).is_empty());
    }

    #[test]
    fn core_archive_is_bounded_and_not_touched_by_reset() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cores: vec![Some(1.0); 500], ..Default::default() });
        assert_eq!(h.archive_cores(None).len(), MAX_CORES);
        h.reset();
        assert_eq!(h.archive_cores(None).len(), MAX_CORES);
    }

    #[test]
    fn core_archive_exports_and_imports_completed_periods() {
        let mut h = History::new();
        h.record_slow(100.0, &Readings { cores: vec![Some(10.0), Some(20.0)], ..Default::default() });
        h.record_slow(110.0, &Readings { cores: vec![Some(30.0), Some(40.0)], ..Default::default() });
        let exported = h.cores_export();
        assert_eq!(exported.len(), 2);
        assert_eq!(exported[0].len(), 1); // la période en cours n'est pas figée
        let mut fresh = History::new();
        fresh.cores_import(exported);
        assert_eq!(fresh.archive_cores(None)[1][0].mean, 20.0);
    }

    struct FourCores;
    impl Sensors for FourCores {
        fn cpu_percent(&mut self) -> Option<f64> { Some(50.0) }
        fn cpu_freq_mhz(&mut self) -> Option<f64> { None }
        fn ram_percent(&mut self) -> Option<f64> { None }
        fn temperature_c(&mut self) -> Option<f64> { None }
        fn cpu_cores_percent(&mut self) -> Vec<Option<f64>> { vec![Some(10.0), Some(20.0), Some(30.0), Some(40.0)] }
        fn fan(&mut self) -> Option<FanReading> { None }
    }

    #[test]
    fn monitor_collects_cores_and_survives_a_panicking_core_reader() {
        let m = Monitor::start(Box::new(FourCores));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(m.history().lock().unwrap().archive_cores(None).len(), 4);
        m.stop();
    }
}
