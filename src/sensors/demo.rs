//! Capteurs synthétiques pour développer hors Raspberry Pi (`--demo`).

use super::{FanReading, Sensors};

const LOAD_PERIOD_S: f64 = 240.0; // cycle charge faible / forte : ventilateur et throttling se déclenchent
const FAN_ON_C: f64 = 52.0;
const THROTTLE_C: f64 = 58.0;

pub struct DemoSensors {
    rng: u64,
    tick: u64,
    cpu: f64,
    temp: f64,
    ram: f64,
}

impl DemoSensors {
    pub fn new(seed: u64) -> Self {
        Self { rng: seed.max(1), tick: 0, cpu: 20.0, temp: 45.0, ram: 30.0 }
    }

    /// xorshift64 : reproductible, sans dépendance. Valeur dans [-1, 1].
    fn noise(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    fn fan_on(&self) -> bool {
        self.temp > FAN_ON_C
    }
}

impl Sensors for DemoSensors {
    fn cpu_percent(&mut self) -> Option<f64> {
        self.tick += 1;
        let phase = self.tick as f64 / LOAD_PERIOD_S * std::f64::consts::TAU;
        let n = self.noise();
        self.cpu = (55.0 + 40.0 * phase.sin() + 8.0 * n).clamp(0.0, 100.0);
        Some(self.cpu)
    }

    fn cpu_freq_mhz(&mut self) -> Option<f64> {
        let n = self.noise();
        Some(1500.0 + 900.0 * self.cpu / 100.0 + 30.0 * n)
    }

    fn ram_percent(&mut self) -> Option<f64> {
        self.ram = (self.ram + self.noise()).clamp(5.0, 95.0);
        Some(self.ram)
    }

    fn temperature_c(&mut self) -> Option<f64> {
        let target = 35.0 + 0.32 * self.cpu;
        let n = self.noise();
        self.temp += (target - self.temp) * 0.05 + 0.3 * n;
        Some(self.temp)
    }

    fn power_w(&mut self) -> Option<f64> {
        let n = self.noise();
        Some(2.5 + 4.0 * self.cpu / 100.0 + 0.1 * n)
    }

    fn throttled(&mut self) -> Option<u32> {
        Some(if self.temp > THROTTLE_C { 0x8 } else { 0 }) // limite de température en cours
    }

    fn fan(&mut self) -> Option<FanReading> {
        let on = self.fan_on();
        let (state, pwm, rpm) = if on { (1, 75, 1700) } else { (0, 0, 0) };
        Some(FanReading { state: Some(state), pwm: Some(pwm), rpm: Some(rpm) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::parse_throttled;

    /// Une seconde de démo : tous les capteurs lus dans l'ordre du collecteur.
    struct Tick {
        cpu: f64,
        freq: f64,
        ram: f64,
        temp: f64,
        power: f64,
        throttled: u32,
        fan: FanReading,
    }

    fn run(seed: u64, n: usize) -> Vec<Tick> {
        let mut s = DemoSensors::new(seed);
        (0..n)
            .map(|_| Tick {
                cpu: s.cpu_percent().unwrap(),
                freq: s.cpu_freq_mhz().unwrap(),
                ram: s.ram_percent().unwrap(),
                temp: s.temperature_c().unwrap(),
                power: s.power_w().unwrap(),
                throttled: s.throttled().unwrap(),
                fan: s.fan().unwrap(),
            })
            .collect()
    }

    #[test]
    fn values_stay_in_plausible_ranges() {
        for t in run(1, 2000) {
            assert!((0.0..=100.0).contains(&t.cpu));
            assert!((5.0..=95.0).contains(&t.ram));
            assert!((1000.0..=2800.0).contains(&t.freq));
            assert!((20.0..=90.0).contains(&t.temp));
            assert!((1.0..=10.0).contains(&t.power));
        }
    }

    #[test]
    fn same_seed_gives_same_series() {
        let a: Vec<f64> = run(7, 50).iter().map(|t| t.cpu).collect();
        let b: Vec<f64> = run(7, 50).iter().map(|t| t.cpu).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn cpu_activity_varies() {
        let ticks = run(1, 600);
        let (lo, hi) = ticks.iter().fold((100.0f64, 0.0f64), |(l, h), t| (l.min(t.cpu), h.max(t.cpu)));
        assert!(hi - lo > 40.0, "cpu trop plat : {lo}..{hi}");
    }

    #[test]
    fn fan_and_throttling_events_both_occur_and_both_stop() {
        let ticks = run(1, 3000);
        let fans: Vec<bool> = ticks.iter().map(|t| t.fan.active().unwrap()).collect();
        assert!(fans.contains(&true) && fans.contains(&false));
        let thr: Vec<bool> = ticks.iter().map(|t| parse_throttled(t.throttled).active()).collect();
        assert!(thr.contains(&true) && thr.contains(&false));
    }

    #[test]
    fn fan_reading_is_coherent_with_its_state() {
        for t in run(1, 1000) {
            let on = t.fan.active().unwrap();
            assert_eq!(on, t.fan.rpm.unwrap() > 0);
            assert_eq!(on, t.fan.pwm.unwrap() > 0);
        }
    }

    #[test]
    fn temperature_follows_load_progressively() {
        let ticks = run(1, 1000);
        let max_step = ticks.windows(2).map(|w| (w[1].temp - w[0].temp).abs()).fold(0.0, f64::max);
        assert!(max_step < 2.0, "température en dents de scie : {max_step}");
    }
}
