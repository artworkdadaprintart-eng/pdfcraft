//! Prepress ▸ Overprint: the press view from `/api/overprint?page=` as an overlay, plus the findings list.
//! (Helper C owns this file.)

use egui::{Color32, CornerRadius, Sense};
use serde_json::Value;

use crate::theme::{self, Tokens};
use crate::{DocView, icons, widgets};

use super::{Load, Section, bbox};

/// Kit colours for PASS / WARN / FAIL (anything else reads as muted).
pub(super) fn status_color(status: &str, t: &Tokens) -> Color32 {
    match status {
        "PASS" | "OK" => Color32::from_rgb(0x0c, 0x87, 0x5d),
        "WARN" => Color32::from_rgb(0xaa, 0x67, 0x00),
        "FAIL" | "ERROR" => Color32::from_rgb(0xd5, 0x41, 0x41),
        _ => t.text_muted,
    }
}

/// A small filled status pill, white text.
pub(super) fn status_pill(ui: &mut egui::Ui, t: &Tokens, status: &str) {
    let font = theme::semibold(10.5);
    let galley = ui.painter().layout_no_wrap(status.to_owned(), font, Color32::WHITE);
    let (rect, _) = ui.allocate_exact_size(galley.size() + egui::vec2(12.0, 4.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), status_color(status, t));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

/// A zoom icon that sends the canvas to `(page0, bbox)`.
pub(super) fn zoom_button(ui: &mut egui::Ui, view: &mut DocView, target: (usize, [f32; 4])) {
    if icons::button(ui, "zoom-in", 22.0, false, "Zoom to it").clicked() {
        view.prepress.zoom_to = Some(target);
    }
}

/// The 0-based page the answer is for (the server is 1-based); `fallback` when it does not say.
pub(super) fn page0(json: &Value, fallback: usize) -> usize {
    json["page"].as_u64().map_or(fallback, |p| (p as usize).saturating_sub(1))
}

#[derive(Debug, PartialEq)]
pub(super) struct Finding {
    pub status: String,
    pub note: String,
    pub area_mm2: Option<f64>,
    /// `None` when the server gave no bbox: listed, not zoomable.
    pub zoom: Option<(usize, [f32; 4])>,
}

/// The findings in server order, each with its zoom target.
pub(super) fn findings_from(json: &Value, page0: usize) -> Vec<Finding> {
    json["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| Finding {
            status: f["status"].as_str().unwrap_or("").to_string(),
            note: f["note"].as_str().or(f["detail"].as_str()).unwrap_or("").to_string(),
            area_mm2: f["area_mm2"].as_f64(),
            zoom: bbox(&f["bbox"]).map(|b| (page0, b)),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Press,
    Screen,
    Art,
}

/// Overlay key, server field, and the mode that shows it.
const VIEWS: [(&str, &str, Mode); 2] = [("overprint:press", "png", Mode::Press), ("overprint:screen", "knockout_png", Mode::Screen)];
const DIFF: &str = "overprint:diff";

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value, ctx: &egui::Context) {
    let page = page0(json, view.current);
    let findings = findings_from(json, page);

    // Header: status, summary, count.
    ui.horizontal_wrapped(|ui| {
        status_pill(ui, t, json["status"].as_str().unwrap_or(""));
        ui.label(egui::RichText::new(json["summary"].as_str().unwrap_or("")).color(t.text));
    });
    ui.label(egui::RichText::new(format!("{} findings", findings.len())).color(t.text_muted));
    ui.add_space(6.0);

    // View pills. The mode lives in egui memory: the panel is the only reader.
    let mode_id = egui::Id::new("prepress-overprint-mode");
    let diff_id = egui::Id::new("prepress-overprint-diff");
    let mut mode = ui.data(|d| d.get_temp::<Mode>(mode_id)).unwrap_or(Mode::Press);
    let mut diff = ui.data(|d| d.get_temp::<bool>(diff_id)).unwrap_or(false);
    let has_diff = json["diff_png"].as_str().is_some_and(|s| !s.is_empty());
    ui.horizontal_wrapped(|ui| {
        for (label, m) in [("Press view", Mode::Press), ("Screen view", Mode::Screen), ("Art only", Mode::Art)] {
            if widgets::pill_button(ui, label, mode == m).clicked() {
                mode = m;
            }
        }
    });
    if has_diff {
        ui.add(widgets::checkbox(&mut diff, "Differences"));
    }
    ui.data_mut(|d| {
        d.insert_temp(mode_id, mode);
        d.insert_temp(diff_id, diff);
    });

    // Fetch what is shown, then switch every page's overlay of each key to match.
    for (key, field, m) in VIEWS {
        let url = json[field].as_str().unwrap_or("");
        if mode == m && !url.is_empty() {
            super::ensure_overlay(view, ctx, key, page, url, Color32::WHITE);
        }
    }
    if has_diff && diff {
        // Already magenta on transparent from the server: shown as is.
        super::ensure_overlay(view, ctx, DIFF, page, json["diff_png"].as_str().unwrap_or(""), Color32::WHITE);
    }
    for o in view.prepress.overlays.iter_mut().filter(|o| o.key.starts_with("overprint:")) {
        o.on = match o.key.as_str() {
            DIFF => diff,
            k => VIEWS.iter().any(|(key, _, m)| *key == k && *m == mode),
        };
    }

    if widgets::pill_button(ui, "Re-run", false).clicked() {
        if let Ok(mut l) = view.prepress.slot(Section::Overprint, view.current).lock() {
            *l = Load::Idle;
        }
        view.prepress.clear_overlays("overprint:");
    }

    // Findings.
    widgets::section_title(ui, &format!("{} findings", findings.len()));
    for (n, f) in findings.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}", n + 1)).color(t.text_faint).monospace());
            status_pill(ui, t, &f.status);
            if let Some(a) = f.area_mm2 {
                ui.label(egui::RichText::new(format!("{a:.1} mm²")).color(t.text));
            }
            if let Some(z) = f.zoom {
                zoom_button(ui, view, z);
            }
        });
        if !f.note.is_empty() {
            ui.label(egui::RichText::new(&f.note).color(t.text_muted).small());
        }
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_keep_order_and_zoom_only_with_bbox() {
        let json = serde_json::json!({"page": 2, "findings": [
            {"status": "FAIL", "note": "white overprint", "bbox": [1, 2, 3, 4], "area_mm2": 79.02},
            {"status": "WARN", "detail": "no box", "bbox": null, "area_mm2": 0.11},
            {"status": "FAIL", "note": "tiny", "bbox": [5, 6, 7, 8]},
        ]});
        let f = findings_from(&json, page0(&json, 9));
        assert_eq!(f.len(), 3);
        assert_eq!(f[0].zoom, Some((1, [1.0, 2.0, 3.0, 4.0])));
        assert_eq!((f[1].status.as_str(), f[1].note.as_str(), f[1].zoom), ("WARN", "no box", None));
        assert_eq!(f[2].zoom, Some((1, [5.0, 6.0, 7.0, 8.0])));
        assert_eq!(f[2].area_mm2, None);
        assert!(findings_from(&serde_json::json!({}), 0).is_empty());
    }
}
