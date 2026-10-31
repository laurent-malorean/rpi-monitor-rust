//! Stockage de l'historique : buffers circulaires, sans dépendance à l'UI.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub minimum: f64,
    pub maximum: f64,
    pub mean: f64,
}

fn valid(values: impl Iterator<Item = f64>) -> Option<Stats> {
    let ok: Vec<f64> = values.filter(|v| !v.is_nan()).collect();
    if ok.is_empty() {
        return None;
    }
    Some(Stats {
        minimum: ok.iter().copied().fold(f64::INFINITY, f64::min),
        maximum: ok.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        mean: ok.iter().sum::<f64>() / ok.len() as f64,
    })
}

/// Buffer circulaire de f64 ; NaN = mesure indisponible.
pub struct RingBuffer {
    data: VecDeque<f64>,
    maxlen: usize,
}

impl RingBuffer {
    pub fn new(maxlen: usize) -> Self {
        Self { data: VecDeque::with_capacity(maxlen.min(4096)), maxlen }
    }

    pub fn add(&mut self, v: Option<f64>) {
        if self.data.len() == self.maxlen {
            self.data.pop_front();
        }
        self.data.push_back(v.unwrap_or(f64::NAN));
    }

    pub fn values(&self) -> Vec<f64> {
        self.data.iter().copied().collect()
    }

    pub fn last(&self) -> Option<f64> {
        self.data.back().copied().filter(|v| !v.is_nan())
    }

    pub fn stats(&self) -> Option<Stats> {
        valid(self.data.iter().copied())
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// Agrège des mesures booléennes par périodes fixes (10 s par défaut).
///
/// Chaque période vaut la fraction de mesures « actives » (0..1). Une seule
/// activation suffit à rendre la valeur > 0 : la période n'est jamais perdue.
pub struct BucketSeries {
    bucket_s: f64,
    maxlen: usize,
    done: VecDeque<f64>,
    bucket_id: Option<i64>,
    active: u32,
    total: u32,
}

impl BucketSeries {
    pub fn new(bucket_s: f64, maxlen: usize) -> Self {
        Self { bucket_s, maxlen, done: VecDeque::new(), bucket_id: None, active: 0, total: 0 }
    }

    fn fraction(&self) -> f64 {
        if self.total == 0 { f64::NAN } else { self.active as f64 / self.total as f64 }
    }

    fn push_done(&mut self, v: f64) {
        if self.done.len() == self.maxlen {
            self.done.pop_front();
        }
        self.done.push_back(v);
    }

    pub fn add(&mut self, ts: f64, active: Option<bool>) {
        let bid = (ts / self.bucket_s).floor() as i64;
        match self.bucket_id {
            None => self.bucket_id = Some(bid),
            Some(cur) if bid > cur => {
                let f = self.fraction();
                self.push_done(f);
                let gap = (bid - cur - 1).min(self.maxlen as i64);
                for _ in 0..gap {
                    self.push_done(f64::NAN); // périodes sans mesure
                }
                self.bucket_id = Some(bid);
                self.active = 0;
                self.total = 0;
            }
            _ => {}
        }
        if let Some(a) = active {
            self.total += 1;
            self.active += a as u32;
        }
    }

    /// Périodes terminées + période en cours (dernier élément).
    pub fn values(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self.done.iter().copied().collect();
        if self.bucket_id.is_some() {
            out.push(self.fraction());
        }
        let skip = out.len().saturating_sub(self.maxlen);
        out.split_off(skip)
    }

    pub fn last(&self) -> Option<f64> {
        self.values().last().copied().filter(|v| !v.is_nan())
    }

    pub fn stats(&self) -> Option<Stats> {
        valid(self.values().into_iter())
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.bucket_id = None;
        self.active = 0;
        self.total = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_is_bounded() {
        let mut rb = RingBuffer::new(3);
        for i in 0..10 {
            rb.add(Some(i as f64));
        }
        assert_eq!(rb.values(), vec![7.0, 8.0, 9.0]);
    }

    #[test]
    fn ring_buffer_stats_ignore_unavailable() {
        let mut rb = RingBuffer::new(10);
        for v in [Some(10.0), None, Some(30.0)] {
            rb.add(v);
        }
        let s = rb.stats().unwrap();
        assert_eq!((s.minimum, s.maximum, s.mean), (10.0, 30.0, 20.0));
        assert!(RingBuffer::new(5).stats().is_none());
    }

    #[test]
    fn ring_buffer_last_none_when_unavailable() {
        let mut rb = RingBuffer::new(5);
        rb.add(Some(1.0));
        rb.add(None);
        assert_eq!(rb.last(), None);
    }

    #[test]
    fn one_second_activation_is_kept_in_ten_second_bucket() {
        let mut b = BucketSeries::new(10.0, 180);
        // 20 mesures à 500 ms sur 10 s, une seule seconde (2 mesures) active
        for i in 0..20 {
            b.add(100.0 + i as f64 * 0.5, Some((8..10).contains(&i)));
        }
        b.add(110.0, Some(false)); // ouvre la période suivante
        let vals = b.values();
        assert_eq!(vals[0], 2.0 / 20.0);
        assert_eq!(vals[1], 0.0);
    }

    #[test]
    fn inactive_bucket_is_zero() {
        let mut b = BucketSeries::new(10.0, 180);
        for i in 0..20 {
            b.add(100.0 + i as f64 * 0.5, Some(false));
        }
        assert_eq!(b.values(), vec![0.0]);
    }

    #[test]
    fn gap_periods_are_unavailable_not_zero() {
        let mut b = BucketSeries::new(10.0, 180);
        b.add(100.0, Some(true));
        b.add(135.0, Some(true));
        let vals = b.values();
        assert_eq!(vals[0], 1.0);
        assert!(vals[1].is_nan() && vals[2].is_nan());
    }

    #[test]
    fn bucket_series_is_bounded_and_clear() {
        let mut b = BucketSeries::new(1.0, 5);
        for t in 0..50 {
            b.add(t as f64, Some(true));
        }
        assert!(b.values().len() <= 5);
        b.clear();
        assert!(b.values().is_empty());
    }
}
