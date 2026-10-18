//! Prepress ▸ Trap: the fixer's trap from `/api/trap?page=&trap_mm=&trap_direction=` as a per-ink overlay
//! (ink the trap adds in that ink's colour, pull-back in grey), the width / direction knobs, the ink table,
//! one finding per seam, and "Trap this file" (`/api/trap_save`, writes `<name>_trapped.pdf` beside the PDF;
//! the original is never written).

use egui::{Color32, CornerRadius, Sense};
use serde_json::Value;

use crate::theme::Tokens;
use crate::{DocView, widgets};

use super::overprint::{page0, status_pill, zoom_button};
use super::{Load, Section, bbox};

const OVERLAY: &str = "trap:overlay";
pub const DIRECTIONS: [&str; 3] = ["auto", "spread", "choke"];

/// The knobs, kept in egui temp data: the panel is the only reader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub mm: f32,
    /// Index into [`DIRECTIONS`].
    pub direction: usize,
}

impl Default for Knobs {
    fn default() -> Self {
        Self { mm: 0.07, direction: 0 }
    }
}

fn knobs_id() -> egui::Id {
    egui::Id::new("prepress-trap-knobs")
}

pub fn knobs(ctx: &egui::Context) -> Knobs {
    ctx.data(|d| d.get_temp::<Knobs>(knobs_id())).unwrap_or_default()
}

/// `/api/trap?page=<1-based>&trap_mm=0.07&trap_direction=auto`.
pub fn url(page0: usize, k: Knobs) -> String {
    format!("{}/trap?page={}&trap_mm={:.2}&trap_direction={}", super::API, page0 + 1, k.mm, DIRECTIONS[k.direction.min(2)])
}

/// `/api/trap_save?trap_mm=0.07&trap_direction=auto` - writes `<name>_trapped.pdf` beside the file.
pub fn save_url(k: Knobs) -> String {
    format!("{}/trap_save?trap_mm={:.2}&trap_direction={}", super::API, k.mm, DIRECTIONS[k.direction.min(2)])
}

#[derive(Debug, PartialEq)]
pub struct Ink {
    pub name: String,
    pub rgb: Color32,
    pub gain_mm2: f64,
    pub loss_mm2: f64,
}

/// The ink table in server order (process inks first, then spots).
pub fn inks_from(json: &Value) -> Vec<Ink> {
    json["inks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let c: Vec<u8> = i["rgb"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v.min(255) as u8).collect();
            Ink {
                name: i["name"].as_str().unwrap_or("?").to_string(),
                rgb: if c.len() == 3 { Color32::from_rgb(c[0], c[1], c[2]) } else { Color32::GRAY },
                gain_mm2: i["gain_mm2"].as_f64().unwrap_or(0.0),
                loss_mm2: i["loss_mm2"].as_f64().unwrap_or(0.0),
            }
        })
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct Seam {
    pub area_mm2: f64,
    pub inks: String,
    pub zoom: Option<(usize, [f32; 4])>,
}

/// One finding per seam, largest first (the server sorts).
pub fn seams_from(json: &Value, page0: usize) -> Vec<Seam> {
    json["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| Seam {
            area_mm2: f["area_mm2"].as_f64().unwrap_or(0.0),
            inks: f["inks"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "),
            zoom: bbox(&f["bbox"]).map(|b| (page0, b)),
        })
        .collect()
}

fn swatch(ui: &mut egui::Ui, rgb: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), rgb);
}

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value, ctx: &egui::Context) {
    let page = page0(json, view.current);
    let inks = inks_from(json);
    let seams = seams_from(json, page);

    ui.horizontal_wrapped(|ui| {
        status_pill(ui, t, json["status"].as_str().unwrap_or(""));
        ui.label(egui::RichText::new(json["summary"].as_str().unwrap_or("")).color(t.text));
    });
    ui.add_space(6.0);

    // Knobs: a change makes the section fetch again with the new query.
    let before = knobs(ctx);
    let mut k = before;
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Trap width").color(t.text_muted));
        ui.add(egui::DragValue::new(&mut k.mm).range(0.02..=0.30).speed(0.005).fixed_decimals(2).suffix(" mm"));
        for (i, d) in DIRECTIONS.iter().enumerate() {
            let label = match *d {
                "auto" => "Auto",
                "spread" => "Spread",
                _ => "Choke",
            };
            if widgets::pill_button(ui, label, k.direction == i).clicked() {
                k.direction = i;
            }
        }
    });
    if k != before {
        ctx.data_mut(|d| d.insert_temp(knobs_id(), k));
        if let Ok(mut l) = view.prepress.slot(Section::Trap, view.current).lock() {
            *l = Load::Idle;
        }
        view.prepress.clear_overlays("trap:");
    }

    // The overlay, togglable.
    let show_id = egui::Id::new("prepress-trap-show");
    let mut show = ui.data(|d| d.get_temp::<bool>(show_id)).unwrap_or(true);
    ui.horizontal_wrapped(|ui| {
        ui.add(widgets::checkbox(&mut show, "Show trap"));
        if widgets::pill_button(ui, "Re-run", false).clicked() {
            if let Ok(mut l) = view.prepress.slot(Section::Trap, view.current).lock() {
                *l = Load::Idle;
            }
            view.prepress.clear_overlays("trap:");
        }
    });
    ui.data_mut(|d| d.insert_temp(show_id, show));
    let png = json["png"].as_str().unwrap_or("");
    if !png.is_empty() {
        super::ensure_overlay(view, ctx, OVERLAY, page, png, Color32::WHITE);
    }
    for o in view.prepress.overlays.iter_mut().filter(|o| o.key == OVERLAY) {
        o.on = show;
    }

    // Trap this file: only on a click, never automatic.
    let save = view.prepress.trap_save.clone();
    let state = save.lock().map(|l| l.clone()).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        let busy = matches!(state, Load::Loading);
        if widgets::pill_button(ui, "Trap this file", true).clicked() && !busy {
            if let Ok(mut l) = save.lock() {
                *l = Load::Idle;
            }
            super::ensure_json(&save, &save_url(k), ctx);
        }
        match &state {
            Load::Idle => {}
            Load::Loading => {
                ui.label(egui::RichText::new("Writing…").color(t.text_muted));
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            Load::Ready(v) => {
                let path = v["path"].as_str().unwrap_or("");
                let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
                ui.label(egui::RichText::new(format!("Written: {name}")).color(Color32::from_rgb(0x0c, 0x87, 0x5d))).on_hover_text(path);
            }
            Load::Failed(e) => {
                ui.colored_label(Color32::from_rgb(0xd5, 0x41, 0x41), e);
            }
        }
    });

    widgets::section_title(ui, "Inks");
    egui::Grid::new("prepress-trap-inks").num_columns(5).spacing([8.0, 2.0]).show(ui, |ui| {
        for (n, i) in inks.iter().enumerate() {
            ui.label(egui::RichText::new(format!("{}", n + 1)).color(t.text_faint).monospace());
            swatch(ui, i.rgb);
            ui.label(egui::RichText::new(&i.name).color(t.text));
            let dim = |v: f64, what: &str| egui::RichText::new(format!("{v:.1} mm² {what}")).color(if v > 0.0 { t.text } else { t.text_faint });
            ui.label(dim(i.gain_mm2, "added"));
            ui.label(dim(i.loss_mm2, "pulled back"));
            ui.end_row();
        }
    });

    widgets::section_title(ui, &format!("{} seams", seams.len()));
    for (n, s) in seams.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}", n + 1)).color(t.text_faint).monospace());
            ui.label(egui::RichText::new(format!("{:.1} mm²", s.area_mm2)).color(t.text));
            ui.label(egui::RichText::new(&s.inks).color(t.text_muted).small());
            if let Some(z) = s.zoom {
                zoom_button(ui, view, z);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_carry_the_knobs() {
        assert_eq!(url(0, Knobs::default()), "/api/trap?page=1&trap_mm=0.07&trap_direction=auto");
        assert_eq!(url(2, Knobs { mm: 0.1, direction: 2 }), "/api/trap?page=3&trap_mm=0.10&trap_direction=choke");
        assert_eq!(save_url(Knobs { mm: 0.125, direction: 1 }), "/api/trap_save?trap_mm=0.12&trap_direction=spread");
        assert_eq!(url(0, Knobs { mm: 0.07, direction: 9 }), url(0, Knobs { mm: 0.07, direction: 2 }), "a bad index clamps");
    }

    #[test]
    fn inks_and_seams_parse_the_server_json() {
        let json = serde_json::json!({"page": 2, "inks": [
            {"name": "Cyan", "rgb": [0, 174, 239], "gain_mm2": 1.5, "loss_mm2": 0.0},
            {"name": "PANTONE Rubine Red C", "rgb": [200, 16, 46], "gain_mm2": 8.66}],
          "findings": [
            {"status": "PASS", "bbox": [1, 2, 3, 4], "area_mm2": 8.66, "inks": ["PANTONE Rubine Red C"], "note": "trap seam"},
            {"status": "PASS", "bbox": null, "area_mm2": 0.2, "inks": ["Cyan", "Black"]}]});
        let inks = inks_from(&json);
        assert_eq!(inks.len(), 2);
        assert_eq!((inks[0].name.as_str(), inks[0].rgb, inks[0].gain_mm2), ("Cyan", Color32::from_rgb(0, 174, 239), 1.5));
        assert_eq!(inks[1].loss_mm2, 0.0);
        let seams = seams_from(&json, page0(&json, 0));
        assert_eq!(seams[0], Seam { area_mm2: 8.66, inks: "PANTONE Rubine Red C".into(), zoom: Some((1, [1.0, 2.0, 3.0, 4.0])) });
        assert_eq!((seams[1].inks.as_str(), seams[1].zoom), ("Cyan, Black", None));
        assert!(inks_from(&serde_json::json!({})).is_empty() && seams_from(&serde_json::json!({}), 0).is_empty());
    }
}
