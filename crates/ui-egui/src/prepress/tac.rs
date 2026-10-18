//! Prepress ▸ Ink coverage (TAC): the heat map from `/api/tac?page=` as an overlay, plus the hotspots.
//! (Helper C owns this file.)

use egui::{Color32, CornerRadius, Sense};
use serde_json::Value;

use crate::theme::Tokens;
use crate::{DocView, widgets};

use super::overprint::{page0, status_color, status_pill, zoom_button};
use super::{Load, Section, bbox};

const KEY: &str = "tac";
/// The art shows through the heat map faintly.
const TINT: Color32 = Color32::from_rgba_premultiplied(200, 200, 200, 200); // white at alpha 200
const MM_PER_PT: f32 = 25.4 / 72.0;

/// Header colour: red over the limit, the status colour otherwise.
pub(super) fn header_color(max: f64, limit: f64, status: &str, t: &Tokens) -> Color32 {
    if max > limit { status_color("FAIL", t) } else { status_color(status, t) }
}

#[derive(Debug, PartialEq)]
pub(super) struct Hotspot {
    pub max_pct: f64,
    pub area_mm2: f64,
    pub zoom: (usize, [f32; 4]),
}

/// Hotspots in server order (largest first); one without a usable bbox is dropped.
pub(super) fn hotspots_from(json: &Value, page0: usize) -> Vec<Hotspot> {
    json["hotspots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| {
            Some(Hotspot {
                max_pct: h["max_pct"].as_f64().or(h["pct"].as_f64()).unwrap_or(0.0),
                area_mm2: h["area_mm2"].as_f64().unwrap_or(0.0),
                zoom: (page0, bbox(&h["bbox"])?),
            })
        })
        .collect()
}

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value, ctx: &egui::Context) {
    let page = page0(json, view.current);
    let max = json["max_pct"].as_f64().unwrap_or(0.0);
    let limit = json["limit"].as_f64().unwrap_or(0.0);
    let status = json["status"].as_str().unwrap_or("");

    ui.horizontal_wrapped(|ui| {
        status_pill(ui, t, status);
        ui.label(egui::RichText::new(format!("max {max:.0} % of limit {limit:.0} %")).strong().color(header_color(max, limit, status, t)));
    });
    if let Some(s) = json["summary"].as_str() {
        ui.label(egui::RichText::new(s).color(t.text_muted));
    }
    ui.add_space(6.0);

    // Heat map tick, default on; remembered in egui memory.
    let id = egui::Id::new("prepress-tac-heat");
    let mut heat = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let png = json["png"].as_str().unwrap_or("");
    if !png.is_empty() {
        ui.add(widgets::checkbox(&mut heat, "Heat map"));
        ui.data_mut(|d| d.insert_temp(id, heat));
        if heat {
            super::ensure_overlay(view, ctx, KEY, page, png, TINT);
        }
        for o in view.prepress.overlays.iter_mut().filter(|o| o.key == KEY) {
            o.on = heat;
        }
    }

    // Legend: a swatch per band.
    ui.horizontal_wrapped(|ui| {
        for band in json["legend"].as_array().into_iter().flatten() {
            let rgb: Vec<u8> = band["rgb"].as_array().into_iter().flatten().filter_map(|c| c.as_u64().map(|c| c as u8)).collect();
            let [r, g, b] = rgb[..] else { continue };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(r, g, b));
            ui.label(egui::RichText::new(format!("{}–{} %", band["from"], band["to"])).small().color(t.text_muted));
            ui.add_space(6.0);
        }
    });
    let counted: Vec<&str> = json["inks_counted"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !counted.is_empty() {
        ui.label(egui::RichText::new(format!("Counted: {}", counted.join(", "))).small().color(t.text_faint));
    }

    if widgets::pill_button(ui, "Re-run", false).clicked() {
        if let Ok(mut l) = view.prepress.slot(Section::Tac, view.current).lock() {
            *l = Load::Idle;
        }
        view.prepress.clear_overlays(KEY);
    }

    let hotspots = hotspots_from(json, page);
    widgets::section_title(ui, &format!("{} hotspots", hotspots.len()));
    for (n, h) in hotspots.iter().enumerate() {
        let [x0, y0, ..] = h.zoom.1;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}", n + 1)).color(t.text_faint).monospace());
            ui.label(egui::RichText::new(format!("{:.0} %", h.max_pct)).strong().color(header_color(h.max_pct, limit, "", t)));
            ui.label(egui::RichText::new(format!("{:.1} mm² at {:.1}, {:.1} mm", h.area_mm2, x0 * MM_PER_PT, y0 * MM_PER_PT)).color(t.text_muted));
            zoom_button(ui, view, h.zoom);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotspots_keep_order_and_drop_boxless() {
        let json = serde_json::json!({"page": 1, "limit": 300, "hotspots": [
            {"bbox": [10, 20, 30, 40], "max_pct": 340.5, "area_mm2": 12.0},
            {"bbox": null, "max_pct": 320.0, "area_mm2": 1.0},
            {"bbox": [1, 2, 3, 4], "max_pct": 310.0, "area_mm2": 0.5},
        ]});
        let h = hotspots_from(&json, page0(&json, 5));
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].zoom, (0, [10.0, 20.0, 30.0, 40.0]));
        assert_eq!(h[1].zoom, (0, [1.0, 2.0, 3.0, 4.0]));
        assert_eq!(h[0].max_pct, 340.5);
    }

    #[test]
    fn header_is_red_over_the_limit() {
        let t = Tokens::for_kind(crate::theme::ThemeKind::Light);
        let red = Color32::from_rgb(0xd5, 0x41, 0x41);
        assert_eq!(header_color(301.0, 300.0, "PASS", &t), red);
        assert_ne!(header_color(300.0, 300.0, "WARN", &t), red);
        assert_eq!(header_color(300.0, 300.0, "WARN", &t), Color32::from_rgb(0xaa, 0x67, 0x00));
    }
}
