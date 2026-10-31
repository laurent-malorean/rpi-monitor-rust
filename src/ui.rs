//! Dessin ratatui : en-tête, sept lignes empilées (valeurs à gauche, courbe à droite), pied de page.

use crate::app::{App, LaneView, Spec, SPECS};
use crate::render::{auto_scale, big_digits, render_line, render_rows, resample};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub const AXIS_W: usize = 6;

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

pub fn draw(frame: &mut Frame, app: &App) {
    let mut constraints = vec![Constraint::Length(1)];
    constraints.extend(SPECS.iter().map(|s| if s.compact { Constraint::Length(5) } else { Constraint::Fill(1) }));
    constraints.push(Constraint::Length(1));
    let areas = Layout::vertical(constraints).split(frame.area());

    let title = format!("RPi Monitor — {}", app.subtitle());
    frame.render_widget(
        Paragraph::new(title).alignment(Alignment::Center).style(Style::new().add_modifier(Modifier::REVERSED)),
        areas[0],
    );
    for ((spec, view), area) in SPECS.iter().zip(app.views()).zip(&areas[1..=SPECS.len()]) {
        draw_lane(frame, spec, view, *area);
    }
    frame.render_widget(footer(), areas[SPECS.len() + 1]);
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
    let key = Style::new().add_modifier(Modifier::BOLD);
    let mut spans = vec![];
    for (k, label) in [("q", "Quitter"), ("p", "Pause"), ("r", "Reset"), ("w", "Fenêtre")] {
        spans.push(Span::styled(format!(" {k} "), key.add_modifier(Modifier::REVERSED)));
        spans.push(Span::raw(format!(" {label} ")));
    }
    Paragraph::new(Line::from(spans))
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
