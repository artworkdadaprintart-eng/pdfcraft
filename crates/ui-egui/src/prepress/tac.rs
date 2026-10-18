//! Prepress ▸ Ink coverage (TAC): the heat map from `/api/tac?page=` as an overlay, plus the hotspots.
//! (Helper C owns this file.)

use serde_json::Value;

use crate::DocView;
use crate::theme::Tokens;
use crate::widgets;

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, _view: &mut DocView, json: &Value, _ctx: &egui::Context) {
    widgets::section_title(ui, &format!("max {:.0}% of {}", json["max_pct"].as_f64().unwrap_or(0.0), json["limit"]));
    ui.label(egui::RichText::new("Heat-map overlay and hotspots come here (helper C).").color(t.text_muted));
}
