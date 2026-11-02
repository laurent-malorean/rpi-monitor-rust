//! Historique long (48 h glissantes), horodaté, par périodes de 10 s.
//! Indépendant de l'historique de la TUI (`history.rs`, 30 min).

use crate::history::Stats;
use std::collections::VecDeque;

/// Une période terminée : valeurs moyenne, minimale et maximale des mesures disponibles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub ts: f64, // début de la période (s, epoch)
    pub mean: f64,
    pub min: f64,
    pub max: f64,
}

struct Open {
    id: i64,
    sum: f64,
    count: u32,
    min: f64,
    max: f64,
}

/// Série glissante : seules les périodes ayant au moins une mesure existent ;
/// une période sans mesure est absente (donnée indisponible), jamais 0.
pub struct Series {
    step_s: f64,
    retention_s: f64,
    done: VecDeque<Point>,
    open: Option<Open>,
}

impl Series {
    pub fn new(step_s: f64, retention_s: f64) -> Self {
        Self { step_s, retention_s, done: VecDeque::new(), open: None }
    }

    fn close(&mut self) {
        if let Some(o) = self.open.take().filter(|o| o.count > 0) {
            self.done.push_back(Point {
                ts: o.id as f64 * self.step_s,
                mean: o.sum / o.count as f64,
                min: o.min,
                max: o.max,
            });
        }
    }

    pub fn add(&mut self, ts: f64, v: Option<f64>) {
        let id = (ts / self.step_s).floor() as i64;
        if self.open.as_ref().is_some_and(|o| id > o.id) {
            self.close();
        }
        let o = self.open.get_or_insert(Open { id, sum: 0.0, count: 0, min: f64::INFINITY, max: f64::NEG_INFINITY });
        if let Some(v) = v {
            o.sum += v;
            o.count += 1;
            o.min = o.min.min(v);
            o.max = o.max.max(v);
        }
        let oldest = ts - self.retention_s;
        while self.done.front().is_some_and(|p| p.ts < oldest) {
            self.done.pop_front();
        }
    }

    /// Périodes terminées + période en cours, à partir de `since` (inclus).
    pub fn points(&self, since: Option<f64>) -> Vec<Point> {
        let current = self.open.as_ref().filter(|o| o.count > 0).map(|o| Point {
            ts: o.id as f64 * self.step_s,
            mean: o.sum / o.count as f64,
            min: o.min,
            max: o.max,
        });
        self.done
            .iter()
            .copied()
            .chain(current)
            .filter(|p| since.is_none_or(|s| p.ts >= s))
            .collect()
    }

    pub fn stats(&self, since: Option<f64>) -> Option<Stats> {
        let p = self.points(since);
        if p.is_empty() {
            return None;
        }
        Some(Stats {
            minimum: p.iter().map(|x| x.min).fold(f64::INFINITY, f64::min),
            maximum: p.iter().map(|x| x.max).fold(f64::NEG_INFINITY, f64::max),
            mean: p.iter().map(|x| x.mean).sum::<f64>() / p.len() as f64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> Series {
        Series::new(10.0, 100.0)
    }

    #[test]
    fn period_aggregates_mean_min_max() {
        let mut s = series();
        for (t, v) in [(100.0, 10.0), (101.0, 30.0), (102.0, 20.0)] {
            s.add(t, Some(v));
        }
        let p = s.points(None);
        assert_eq!(p, vec![Point { ts: 100.0, mean: 20.0, min: 10.0, max: 30.0 }]);
    }

    #[test]
    fn unavailable_measure_is_not_zero() {
        let mut s = series();
        s.add(100.0, Some(10.0));
        s.add(101.0, None);
        assert_eq!(s.points(None)[0].mean, 10.0);
    }

    #[test]
    fn period_without_any_measure_has_no_point() {
        let mut s = series();
        s.add(100.0, None);
        s.add(110.0, Some(5.0));
        let p = s.points(None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].ts, 110.0);
    }

    #[test]
    fn current_period_is_included() {
        let mut s = series();
        s.add(100.0, Some(1.0));
        s.add(110.0, Some(2.0));
        assert_eq!(s.points(None).iter().map(|p| p.ts).collect::<Vec<_>>(), [100.0, 110.0]);
    }

    #[test]
    fn retention_slides_with_time() {
        let mut s = series(); // rétention 100 s
        for t in (0..300).step_by(10) {
            s.add(t as f64, Some(1.0));
        }
        let p = s.points(None);
        assert!(p.first().unwrap().ts >= 290.0 - 100.0);
        assert_eq!(p.last().unwrap().ts, 290.0);
    }

    #[test]
    fn since_filters_and_stats_cover_the_selection() {
        let mut s = series();
        s.add(100.0, Some(10.0));
        s.add(110.0, Some(50.0));
        s.add(120.0, Some(30.0));
        assert_eq!(s.points(Some(110.0)).len(), 2);
        let st = s.stats(Some(110.0)).unwrap();
        assert_eq!((st.minimum, st.maximum, st.mean), (30.0, 50.0, 40.0));
        assert!(Series::new(10.0, 100.0).stats(None).is_none());
    }
}
