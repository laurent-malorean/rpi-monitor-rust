//! Rendu : fonctions pures de tracé (aucune dépendance au terminal ni à ratatui).

pub const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub const BRAILLE_BLANK: char = '\u{2800}';

/// Bornes (bas, haut) avec amplitude minimale et marge de 15 %.
pub fn auto_scale(values: &[f64], min_span: f64) -> (f64, f64) {
    let ok = || values.iter().copied().filter(|v| !v.is_nan());
    if ok().next().is_none() {
        return (0.0, min_span);
    }
    let lo = ok().fold(f64::INFINITY, f64::min);
    let hi = ok().fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(min_span);
    let pad = span * 0.15;
    let mid = (hi + lo) / 2.0;
    (mid - span / 2.0 - pad, mid + span / 2.0 + pad)
}

/// Ramène les `n_points` dernières valeurs sur `width` colonnes (moyenne ou max).
pub fn resample(values: &[f64], n_points: usize, width: usize, use_max: bool) -> Vec<f64> {
    let tail = &values[values.len().saturating_sub(n_points)..];
    let mut pts = vec![f64::NAN; n_points - tail.len()];
    pts.extend_from_slice(tail);
    (0..width)
        .map(|c| {
            let a = c * n_points / width;
            let b = ((c + 1) * n_points / width).max(a + 1);
            let ok: Vec<f64> = pts[a..b].iter().copied().filter(|v| !v.is_nan()).collect();
            if ok.is_empty() {
                f64::NAN
            } else if use_max {
                ok.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            } else {
                ok.iter().sum::<f64>() / ok.len() as f64
            }
        })
        .collect()
}

/// Tracé en blocs 1/8 : `height` lignes de `width` colonnes, plus récent à droite.
pub fn render_rows(values: &[f64], width: usize, height: usize, lo: f64, hi: f64, min_level: u32) -> Vec<String> {
    let tail = &values[values.len().saturating_sub(width)..];
    let mut cols = vec![f64::NAN; width - tail.len()];
    cols.extend_from_slice(tail);
    let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
    let max_level = (height * 8) as i64;
    let levels: Vec<i64> = cols
        .iter()
        .map(|&v| {
            if v.is_nan() {
                return 0;
            }
            let lvl = (((v - lo) / span) * max_level as f64).round() as i64;
            let lvl = lvl.clamp(0, max_level);
            if min_level > 0 && v > lo { lvl.max(i64::from(min_level)) } else { lvl } // activation brève visible
        })
        .collect();
    (0..height)
        .rev()
        .map(|r| levels.iter().map(|&lv| BLOCKS[(lv - r as i64 * 8).clamp(0, 8) as usize]).collect())
        .collect()
}

const LEFT: [u32; 4] = [0x01, 0x02, 0x04, 0x40]; // bits braille, colonne gauche, de haut en bas
const RIGHT: [u32; 4] = [0x08, 0x10, 0x20, 0x80];

/// Courbe en braille (2x4 points par cellule). `values` : 2*width points.
pub fn render_line(values: &[f64], width: usize, height: usize, lo: f64, hi: f64) -> Vec<String> {
    let dots_h = height * 4;
    let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
    let mut cells = vec![vec![0u32; width]; height];
    let mut mark = |x: usize, y: usize| {
        let bits = if x % 2 == 1 { RIGHT } else { LEFT };
        cells[y / 4][x / 2] |= bits[y % 4];
    };
    let mut prev: Option<usize> = None;
    for (x, &v) in values.iter().take(width * 2).enumerate() {
        if v.is_nan() {
            prev = None;
            continue;
        }
        let level = (((v - lo) / span) * (dots_h - 1) as f64).round().clamp(0.0, (dots_h - 1) as f64) as usize;
        let y = dots_h - 1 - level;
        let (from, to) = prev.map_or((y, y), |p| (y.min(p), y.max(p)));
        (from..=to).for_each(|yy| mark(x, yy)); // relie au point précédent : courbe continue
        prev = Some(y);
    }
    cells
        .into_iter()
        .map(|row| row.into_iter().map(|c| char::from_u32(0x2800 + c).unwrap_or(BRAILLE_BLANK)).collect())
        .collect()
}

/// Superpose des couches braille de même taille : les points d'une cellule sont réunis, et la
/// cellule revient à la couche qui y a le plus de points (à égalité, la dernière). `None` : cellule vide.
pub fn merge_braille(layers: &[Vec<String>]) -> Vec<Vec<(char, Option<usize>)>> {
    let grids: Vec<Vec<Vec<char>>> = layers.iter().map(|l| l.iter().map(|r| r.chars().collect()).collect()).collect();
    let rows = grids.iter().map(Vec::len).max().unwrap_or(0);
    (0..rows)
        .map(|r| {
            let cols = grids.iter().filter_map(|g| g.get(r)).map(Vec::len).max().unwrap_or(0);
            (0..cols)
                .map(|c| {
                    let (mut bits, mut owner, mut best) = (0u32, None, 0);
                    for (i, g) in grids.iter().enumerate() {
                        let b = g.get(r).and_then(|row| row.get(c)).map_or(0, |ch| (*ch as u32).saturating_sub(0x2800) & 0xFF);
                        bits |= b;
                        if b.count_ones() > 0 && b.count_ones() >= best {
                            best = b.count_ones();
                            owner = Some(i);
                        }
                    }
                    (if bits == 0 { ' ' } else { char::from_u32(0x2800 + bits).unwrap_or(' ') }, owner)
                })
                .collect()
        })
        .collect()
}

/// Glyphes 3 lignes (chiffres, point, tiret) pour les gros nombres.
fn glyph(c: char) -> [&'static str; 3] {
    match c {
        '0' => ["┏━┓", "┃ ┃", "┗━┛"],
        '1' => [" ╻ ", " ┃ ", " ╹ "],
        '2' => ["╺━┓", "┏━┛", "┗━╸"],
        '3' => ["╺━┓", " ━┫", "╺━┛"],
        '4' => ["╻ ╻", "┗━┫", "  ╹"],
        '5' => ["┏━╸", "┗━┓", "╺━┛"],
        '6' => ["┏━╸", "┣━┓", "┗━┛"],
        '7' => ["╺━┓", "  ┃", "  ╹"],
        '8' => ["┏━┓", "┣━┫", "┗━┛"],
        '9' => ["┏━┓", "┗━┫", "╺━┛"],
        '.' => [" ", " ", "."],
        '-' => ["   ", " ━ ", "   "],
        _ => [" ", " ", " "],
    }
}

/// Gros chiffres sur 3 lignes, de même largeur.
pub fn big_digits(text: &str) -> [String; 3] {
    let mut out = [String::new(), String::new(), String::new()];
    for c in text.chars() {
        for (row, part) in out.iter_mut().zip(glyph(c)) {
            row.push_str(part);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    #[test]
    fn render_rows_shape_and_scale() {
        let rows = render_rows(&[0.0, 50.0, 100.0], 5, 2, 0.0, 100.0, 0);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.chars().count() == 5));
        assert_eq!(rows[0].chars().last(), Some('█')); // 100 % remplit tout
        assert_eq!(rows[1].chars().last(), Some('█'));
    }

    #[test]
    fn render_rows_unavailable_is_blank_and_recent_is_on_the_right() {
        let rows = render_rows(&[NAN, 100.0], 4, 1, 0.0, 100.0, 0);
        assert_eq!(rows[0], "   █");
    }

    #[test]
    fn render_rows_min_level_keeps_brief_activation_visible() {
        let tiny = render_rows(&[0.001], 1, 4, 0.0, 1.0, 3);
        assert_eq!(tiny[3], "▃"); // niveau 3 sur la ligne du bas
        let none = render_rows(&[0.0], 1, 4, 0.0, 1.0, 3);
        assert_eq!(none[3], " "); // inactif : rien
    }

    #[test]
    fn temperature_scale_can_go_below_30() {
        let (lo, hi) = auto_scale(&[22.0, 25.0], 10.0);
        assert!(lo < 22.0 && hi > 25.0);
    }

    #[test]
    fn auto_scale_enforces_min_span_and_handles_no_data() {
        let (lo, hi) = auto_scale(&[50.0, 50.0], 10.0);
        assert!(hi - lo >= 10.0 && lo < 50.0 && hi > 50.0);
        assert_eq!(auto_scale(&[NAN], 10.0), (0.0, 10.0));
        assert_eq!(auto_scale(&[], 10.0), (0.0, 10.0));
    }

    #[test]
    fn resample_window_fills_width() {
        let out = resample(&[1.0, 3.0], 4, 4, false);
        assert_eq!(out.len(), 4);
        assert_eq!(out[3], 3.0);
        assert_eq!(out[2], 1.0);
        assert!(out[0].is_nan());
        assert_eq!(resample(&[0.0, 1.0, 0.0, 0.0], 4, 2, true), vec![1.0, 0.0]);
    }

    #[test]
    fn resample_averages_and_keeps_only_the_window() {
        assert_eq!(resample(&[9.0, 9.0, 1.0, 3.0], 2, 1, false), vec![2.0]); // fenêtre = 2 derniers
        assert!(resample(&[NAN, NAN], 2, 1, false)[0].is_nan());
    }

    #[test]
    fn render_line_is_a_curve_not_a_fill() {
        let rows = render_line(&[10.0; 8], 4, 2, 0.0, 100.0);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.chars().count() == 4));
        assert!(rows[0].chars().all(|c| c == BRAILLE_BLANK)); // valeur basse : rien en haut
        let mut v = vec![0.0, 100.0];
        v.extend([NAN; 6]);
        let rows = render_line(&v, 4, 2, 0.0, 100.0);
        assert_ne!(rows[0].chars().next(), Some(BRAILLE_BLANK)); // segment vertical relié
        assert_ne!(rows[1].chars().next(), Some(BRAILLE_BLANK));
    }

    #[test]
    fn render_line_breaks_on_unavailable_values() {
        let rows = render_line(&[50.0, NAN, 50.0, NAN], 2, 1, 0.0, 100.0);
        assert!(rows[0].chars().all(|c| c != BRAILLE_BLANK));
        let blank = render_line(&[NAN; 4], 2, 1, 0.0, 100.0);
        assert!(blank[0].chars().all(|c| c == BRAILLE_BLANK));
    }

    #[test]
    fn big_digits_are_three_rows_of_equal_width() {
        let d = big_digits("12.5");
        assert!(d.iter().all(|r| r.chars().count() == d[0].chars().count()));
        assert!(d[0].chars().count() >= 4);
        assert_ne!(big_digits("0"), big_digits("8"));
        let dash = big_digits("--");
        assert!(dash.iter().any(|r| r.contains('━')));
    }

    #[test]
    fn merge_braille_combines_dots_and_credits_the_layer_with_most_dots() {
        let a = vec!["\u{2801}\u{2801}".to_string()];
        let b = vec!["\u{2802}\u{2807}".to_string()];
        let m = merge_braille(&[a, b]);
        assert_eq!(m[0][0], ('\u{2803}', Some(1)), "points réunis dans la cellule, 1 point chacun : le dernier l'emporte");
        assert_eq!(m[0][1], ('\u{2807}', Some(1)), "la couche aux plus de points l'emporte");
    }

    #[test]
    fn merge_braille_keeps_blank_cells_blank_and_cells_of_a_single_layer() {
        let a = vec!["\u{2800}\u{2840}".to_string()];
        let b = vec!["\u{2800}\u{2800}".to_string()];
        let m = merge_braille(&[a, b]);
        assert_eq!(m[0][0], (' ', None));
        assert_eq!(m[0][1], ('\u{2840}', Some(0)));
    }

    #[test]
    fn merge_braille_tolerates_no_layer_or_ragged_layers() {
        assert!(merge_braille(&[]).is_empty());
        let m = merge_braille(&[vec!["\u{2801}".to_string()], vec!["\u{2801}\u{2801}".to_string(), "\u{2801}".to_string()]]);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].len(), 2);
    }
}
