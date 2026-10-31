//! Capteurs : interface commune, types de mesure et implémentations (réelle, démo).
//! Lecture seule ; `None` = mesure indisponible.

pub mod demo;
pub mod linux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanReading {
    pub state: Option<i64>, // niveau demandé par le système (cooling_device cur_state)
    pub pwm: Option<i64>,   // consigne PWM 0..255
    pub rpm: Option<i64>,   // vitesse mesurée
}

impl FanReading {
    /// Actif si l'un des signaux connus est > 0 ; `None` si aucun n'est lisible.
    pub fn active(&self) -> Option<bool> {
        let vals: Vec<i64> = [self.state, self.pwm, self.rpm].into_iter().flatten().collect();
        if vals.is_empty() {
            None
        } else {
            Some(vals.iter().any(|&v| v > 0))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Throttle {
    pub now: Vec<&'static str>,        // en cours
    pub since_boot: Vec<&'static str>, // survenus depuis le démarrage
}

impl Throttle {
    pub fn active(&self) -> bool {
        !self.now.is_empty()
    }
}

const FLAGS: [(u32, &str); 4] =
    [(0, "sous-tension"), (1, "fréq. bridée"), (2, "throttle"), (3, "limite temp")];
const PAST_SHIFT: u32 = 16; // bits 16..19 : même événement, survenu depuis le boot

/// Décode le registre `get_throttled` du firmware Pi.
pub fn parse_throttled(raw: u32) -> Throttle {
    let pick = |shift: u32| FLAGS.iter().filter(|(b, _)| raw >> (b + shift) & 1 == 1).map(|&(_, n)| n).collect();
    Throttle { now: pick(0), since_boot: pick(PAST_SHIFT) }
}

/// Capteurs d'une machine (réelle ou démo).
pub trait Sensors: Send {
    fn cpu_percent(&mut self) -> Option<f64>;
    fn cpu_freq_mhz(&mut self) -> Option<f64>;
    fn ram_percent(&mut self) -> Option<f64>;
    fn temperature_c(&mut self) -> Option<f64>;
    fn power_w(&mut self) -> Option<f64> { None }
    fn throttled(&mut self) -> Option<u32> { None }
    fn fan(&mut self) -> Option<FanReading>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_active_if_any_signal_positive() {
        let f = |s, p, r| FanReading { state: s, pwm: p, rpm: r }.active();
        assert_eq!(f(Some(0), Some(0), Some(0)), Some(false));
        assert_eq!(f(Some(1), Some(0), Some(0)), Some(true)); // tourne alors que pwm/rpm = 0
        assert_eq!(f(None, None, None), None);
    }

    #[test]
    fn parse_throttled_ok() {
        let t = parse_throttled(0);
        assert!(t.now.is_empty() && t.since_boot.is_empty() && !t.active());
    }

    #[test]
    fn parse_throttled_now_and_since_boot() {
        // 0x50005 : sous-tension + throttle en cours, déjà survenus depuis le boot
        let t = parse_throttled(0x50005);
        assert_eq!(t.now, ["sous-tension", "throttle"]);
        assert_eq!(t.since_boot, ["sous-tension", "throttle"]);
        assert!(t.active());
    }

    #[test]
    fn parse_throttled_past_only_is_not_active() {
        let t = parse_throttled(0x80000); // limite de température survenue, plus en cours
        assert!(t.now.is_empty() && !t.active());
        assert_eq!(t.since_boot, ["limite temp"]);
    }
}
