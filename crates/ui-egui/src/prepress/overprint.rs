//! Prepress ▸ Overprint: the press view from `/api/overprint?page=` as an overlay, plus the findings list.
//! (Helper C owns this file.)

use serde_json::Value;

use crate::DocView;
use crate::theme::Tokens;
use crate::widgets;

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, _view: &mut DocView, json: &Value, _ctx: &egui::Context) {
    let n = json["findings"].as_array().map_or(0, Vec::len);
    widgets::section_title(ui, &format!("{n} findings · {}", json["status"].as_str().unwrap_or("")));
    ui.label(egui::RichText::new("Press-view overlay and the findings list come here (helper C).").color(t.text_muted));
}
