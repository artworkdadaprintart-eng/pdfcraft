//! Prepress ▸ Separations: plates from `/api/separations?page=` as toggleable overlays, plus the ink table.
//! (Helper B owns this file.) The server's plate PNGs are already ink-coloured RGBA with alpha = ink
//! amount, so they alpha-blend over the art as they are: no remap needed.

use egui::Color32;
use serde_json::Value;

use crate::DocView;
use crate::theme::Tokens;
use crate::widgets;

use super::{Load, Section, bbox, ensure_overlay};

const RED: Color32 = Color32::from_rgb(0xD5, 0x41, 0x41);

fn ink_rgb(ink: &Value) -> Color32 {
    let c: Vec<u8> = ink["rgb"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(128).min(255) as u8).collect()).unwrap_or_default();
    if c.len() == 3 { Color32::from_rgb(c[0], c[1], c[2]) } else { Color32::GRAY }
}

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value, ctx: &egui::Context) {
    let page = view.current;
    let inks: &[Value] = json["inks"].as_array().map_or(&[], Vec::as_slice);
    // Fetch each plate once; they appear as they arrive.
    for ink in inks {
        if let (Some(name), Some(url)) = (ink["name"].as_str(), ink["png"].as_str()) {
            ensure_overlay(view, ctx, &format!("sep:{name}"), page, url, Color32::WHITE);
        }
    }
    widgets::section_title(ui, &format!("{} inks · page {}", inks.len(), page + 1));
    ui.horizontal_wrapped(|ui| {
        for (label, on) in [("All", true), ("None", false)] {
            if widgets::pill_button(ui, label, false).clicked() {
                for o in view.prepress.overlays.iter_mut().filter(|o| o.page == page && o.key.starts_with("sep:")) {
                    o.on = on;
                }
            }
        }
        if widgets::pill_button(ui, "Re-run", false).clicked() {
            view.prepress.clear_overlays("sep:");
            let slot = view.prepress.slot(Section::Separations, page);
            if let Ok(mut l) = slot.lock() {
                *l = Load::Idle;
            }
        }
    });
    ui.add_space(4.0);
    egui::Grid::new("sep-table").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        for (n, ink) in inks.iter().enumerate() {
            let name = ink["name"].as_str().unwrap_or("?");
            let key = format!("sep:{name}");
            ui.label(egui::RichText::new((n + 1).to_string()).color(t.text_faint));
            match view.prepress.overlay_mut(&key, page) {
                Some(o) => {
                    ui.add(crate::widgets::checkbox(&mut o.on, ""));
                }
                None => {
                    ui.add_enabled(false, crate::widgets::checkbox(&mut true, ""));
                }
            }
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 3.0, ink_rgb(ink));
            ui.painter().rect_stroke(r, 3.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).color(t.text));
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {:.1}% coverage",
                        ink["kind"].as_str().unwrap_or(""),
                        ink["coverage_pct"].as_f64().unwrap_or(0.0)
                    ))
                    .small()
                    .color(t.text_muted),
                );
                if let Some(a) = ink["alarm"].as_str().filter(|a| !a.is_empty()) {
                    ui.add(egui::Label::new(egui::RichText::new(a).small().strong().color(RED)).wrap());
                }
            });
            ui.end_row();
        }
    });
    // Alarms: the point of this panel, in red and zoomable.
    let alarms: &[Value] = json["alarms"].as_array().map_or(&[], Vec::as_slice);
    widgets::section_title(ui, &format!("{} alarm{}", alarms.len(), if alarms.len() == 1 { "" } else { "s" }));
    for (n, a) in alarms.iter().enumerate() {
        let fail = a["status"].as_str() == Some("FAIL");
        let colour = if fail { RED } else { Color32::from_rgb(0xD9, 0x8E, 0x1F) };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} {} {}: {}",
                    n + 1,
                    a["status"].as_str().unwrap_or(""),
                    a["ink"].as_str().unwrap_or(""),
                    a["note"].as_str().unwrap_or("")
                ))
                .color(colour)
                .strong(),
            );
            if let Some(bb) = bbox(&a["bbox"])
                && widgets::pill_button(ui, "Zoom", false).clicked()
            {
                view.prepress.zoom_to = Some((page, bb));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_rgb_reads_three_bytes_else_grey() {
        assert_eq!(ink_rgb(&serde_json::json!({"rgb": [255, 0, 140]})), Color32::from_rgb(255, 0, 140));
        assert_eq!(ink_rgb(&serde_json::json!({"rgb": [1, 2]})), Color32::GRAY);
        assert_eq!(ink_rgb(&serde_json::json!({})), Color32::GRAY);
    }
}
