//! Dessin ratatui : en-tête, sept lignes empilées (valeurs à gauche, courbe à droite), pied de page.

use crate::app::{App, LaneView, Spec, SPECS};
use crate::render::{auto_scale, big_digits, render_line, render_rows, resample};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub const AXIS_W: usize = 6;
const COMPACT_H: u16 = 5;

/// Lignes du graphique d'un indicateur (axe + tracé), `w` x `h` cellules.
pub fn chart_lines(spec: &Spec, values: &[f64], window_s: f64, w: usize, h: usize) -> Vec<Line<'static>> {
    if w <= AXIS_W + 2 || h < 2 {
        return vec![];
    }
    let n_points = ((window_s / spec.step_s) as usize).max(1);
    let pw = w - AXIS_W;
    let cols = resample(values, n_points, if spec.bars { pw } else { pw * 2 }, spec.bars);
    let (lo, hi) = spec.fixed.unwrap_or_else(|| auto_scale(&cols, spec.min_span));
    let rows = if spec.bars {
        render_rows(&cols, pw, h, lo, hi, 3)
    } else {
        render_line(&cols, pw, h, lo, hi)
    };
    let decimals = if spec.fixed.is_some() || hi - lo > 20.0 { 0 } else { 1 };
    let label = |v: f64| format!("{v:.decimals$}");
    rows.into_iter()
        .enumerate()
        .map(|(i, row)| {
            let axis = match (spec.bars, i) {
                (true, 0) => "ON".to_string(),
                (true, i) if i == h - 1 => "OFF".to_string(),
                (false, 0) => label(hi),
                (false, i) if i == h - 1 => label(lo),
                _ => String::new(),
            };
            Line::from(vec![
                Span::styled(format!("{axis:>w$} ", w = AXIS_W - 1), Style::new().add_modifier(Modifier::DIM)),
                Span::styled(row, Style::new().fg(spec.color)),
            ])
        })
        .collect()
}

pub const BAR_BG: Color = Color::Rgb(0, 0, 128); // barre d'en-tête / pied de page (comme Textual)
pub const KEY_FG: Color = Color::Rgb(255, 170, 0);

/// Répartit `total` lignes sur `n` zones égales ; le reste est cumulé vers le bas :
/// les premières zones sont les plus courtes (même répartition que Textual).
pub fn split_heights(total: u16, n: usize) -> Vec<u16> {
    let edge = |i: usize| (i as u32 * u32::from(total) / n.max(1) as u32) as u16;
    (0..n).map(|i| edge(i + 1) - edge(i)).collect()
}

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    // lignes à hauteur variable : répartition cumulée ; la ligne compacte garde 5 lignes
    let compact = SPECS.iter().filter(|s| s.compact).count();
    let flex_h = body.height.saturating_sub(COMPACT_H * compact as u16);
    let mut flex = split_heights(flex_h, SPECS.len() - compact).into_iter();
    let constraints: Vec<Constraint> = SPECS
        .iter()
        .map(|s| Constraint::Length(if s.compact { COMPACT_H } else { flex.next().unwrap_or(0) }))
        .collect();
    let lanes = Layout::vertical(constraints).split(body);

    let title = format!("RPi Monitor — {}", app.subtitle());
    frame.render_widget(
        Paragraph::new(title).alignment(Alignment::Center).style(Style::new().fg(Color::White).bg(BAR_BG)),
        header,
    );
    for ((spec, view), area) in SPECS.iter().zip(app.views()).zip(lanes.iter()) {
        draw_lane(frame, spec, view, *area);
    }
    frame.render_widget(footer(), footer_area);
}

fn draw_lane(frame: &mut Frame, spec: &Spec, view: &LaneView, area: Rect) {
    let [left, chart] = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)]).areas(area);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut lines = vec![Line::styled(format!("{} ({})", spec.title, spec.unit), Style::new().add_modifier(Modifier::BOLD))];
    if !spec.compact {
        lines.extend(big_digits(&view.big).map(Line::from));
    }
    lines.push(Line::styled(view.stats.clone(), dim));
    frame.render_widget(Paragraph::new(lines), left.inner(Margin::new(1, 0)));
    let chart_lines = chart_lines(spec, &view.values, view.window_s, chart.width as usize, chart.height as usize);
    frame.render_widget(Paragraph::new(chart_lines), chart);
}

fn footer() -> Paragraph<'static> {
    let key = Style::new().fg(KEY_FG).bg(BAR_BG).add_modifier(Modifier::BOLD);
    let label = Style::new().fg(Color::White).bg(BAR_BG);
    let mut spans = vec![];
    for (k, text) in [("q", "Quitter"), ("p", "Pause"), ("r", "Reset"), ("w", "Fenêtre")] {
        spans.push(Span::styled(format!(" {k}"), key));
        spans.push(Span::styled(format!(" {text} "), label));
    }
    Paragraph::new(Line::from(spans)).style(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SPECS;
    use crate::collector::{History, Readings};
    use crate::sensors::{parse_throttled, FanReading};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::sync::{Arc, Mutex};

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn bars_chart_labels_on_off_and_latest_value_on_the_right() {
        let lines = text(&chart_lines(&SPECS[5], &[0.0, 1.0], 60.0, 12, 3));
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.chars().count() == 12));
        assert!(lines[0].trim_start().starts_with("ON"));
        assert!(lines[2].trim_start().starts_with("OFF"));
        assert!(lines[0].ends_with('█')); // 1 = actif, plus récent à droite
    }

    #[test]
    fn line_chart_labels_scale_bounds() {
        let lines = text(&chart_lines(&SPECS[0], &[10.0, 50.0], 60.0, 20, 4));
        assert!(lines[0].trim_start().starts_with("100"));
        assert!(lines[3].trim_start().starts_with('0'));
        assert!(lines.iter().any(|l| l.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))));
    }

    #[test]
    fn heights_sum_to_total_and_differ_by_at_most_one() {
        for (total, n) in [(35, 6), (40, 6), (12, 6), (5, 6), (0, 6), (7, 1)] {
            let h = split_heights(total, n);
            assert_eq!(h.len(), n);
            assert_eq!(h.iter().sum::<u16>(), total);
            assert!(h.iter().max().unwrap() - h.iter().min().unwrap() <= 1);
        }
    }

    #[test]
    fn short_lane_comes_first_like_textual() {
        assert_eq!(split_heights(35, 6), vec![5, 6, 6, 6, 6, 6]);
    }

    #[test]
    fn header_and_footer_use_the_blue_bar_style() {
        let app = App::new(history(), false);
        let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        assert_eq!(buf[(0, 0)].bg, BAR_BG);
        assert_eq!(buf[(99, 0)].bg, BAR_BG);
        assert_eq!(buf[(0, 39)].bg, BAR_BG);
        let key = (0..100).map(|x| &buf[(x, 39)]).find(|c| c.symbol() == "q").unwrap();
        assert_eq!(key.fg, KEY_FG);
    }

    #[test]
    fn chart_too_small_is_empty() {
        assert!(chart_lines(&SPECS[0], &[1.0], 60.0, AXIS_W + 2, 4).is_empty());
        assert!(chart_lines(&SPECS[0], &[1.0], 60.0, 40, 1).is_empty());
    }

    fn history() -> Arc<Mutex<History>> {
        let h = Arc::new(Mutex::new(History::new()));
        {
            let mut g = h.lock().unwrap();
            for i in 0..30 {
                let r = Readings {
                    cpu: Some(20.0 + i as f64),
                    freq: Some(1800.0),
                    power: Some(4.2),
                    ram: Some(33.0),
                    temp: Some(50.0),
                    throttle: Some(parse_throttled(0)),
                };
                g.record_slow(100.0 + i as f64, &r);
                g.record_fast(100.0 + i as f64, Some(FanReading { state: Some(1), pwm: Some(75), rpm: Some(1700) }));
            }
        }
        h
    }

    fn screen(app: &App, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect()
    }

    #[test]
    fn all_seven_indicators_are_stacked_in_order() {
        let app = App::new(history(), true);
        let s = screen(&app, 110, 45);
        let rows: Vec<usize> = ["CPU (%)", "Fréquence (MHz)", "Consommation (W)", "RAM (%)", "Température (°C)",
            "Ventilateur (tr/min)", "Throttling (flags)"]
            .iter()
            .map(|t| s.iter().position(|l| l.contains(t)).unwrap_or_else(|| panic!("{t} absent")))
            .collect();
        assert!(rows.windows(2).all(|w| w[0] < w[1]), "ordre : {rows:?}");
    }

    #[test]
    fn header_footer_big_digits_and_curve_are_drawn() {
        let app = App::new(history(), true);
        let s = screen(&app, 110, 45).join("\n");
        assert!(s.contains("RPi Monitor") && s.contains("DEMO"));
        assert!(s.contains("Quitter") && s.contains("Pause") && s.contains("Reset") && s.contains("Fenêtre"));
        assert!(s.contains("┏━┓")); // gros chiffres
        assert!(s.contains("min 20 max 49 moy 34")); // statistiques CPU
        assert!(s.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))); // courbe
    }

    #[test]
    fn renders_at_every_terminal_height_without_panic() {
        let app = App::new(history(), false);
        for h in [60, 45, 40, 30, 20, 12, 6, 3, 1] {
            let s = screen(&app, 100, h);
            assert_eq!(s.len(), h as usize);
        }
        for w in [100, 60, 40, 10, 1] {
            screen(&app, w, 40);
        }
    }

    #[test]
    fn short_terminal_keeps_header_footer_and_throttle_lane() {
        let app = App::new(history(), false);
        let s = screen(&app, 100, 36).join("\n");
        assert!(s.contains("Quitter") && s.contains("Throttling"));
    }
}
