//! Prepress ▸ Separations: plates from `/api/separations?page=` as toggleable overlays, plus the ink table.
//! (Helper B owns this file.)

use serde_json::Value;

use crate::DocView;
use crate::theme::Tokens;
use crate::widgets;

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, _view: &mut DocView, json: &Value, _ctx: &egui::Context) {
    let inks = json["inks"].as_array().map_or(0, Vec::len);
    widgets::section_title(ui, &format!("{inks} inks"));
    ui.label(egui::RichText::new("Plates as overlays and the ink table come here (helper B).").color(t.text_muted));
}
