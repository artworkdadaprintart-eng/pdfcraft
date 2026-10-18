//! Prepress ▸ Checks: the 16 rows from `/api/checks`, each item zooming the page to its bbox.
//! (Helper A owns this file.)

use serde_json::Value;

use crate::DocView;
use crate::theme::Tokens;
use crate::widgets;

use super::bbox;

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value) {
    let rows = json["checks"].as_array().map_or(0, Vec::len);
    let verdict = json["verdict"].as_str().unwrap_or("");
    widgets::section_title(ui, &format!("{rows} checks · {verdict}"));
    for (n, row) in json["checks"].as_array().into_iter().flatten().enumerate() {
        let name = row["check"].as_str().unwrap_or("?");
        let status = row["status"].as_str().unwrap_or("");
        let resp = ui.selectable_label(false, format!("{} {status} {name}", n + 1));
        // First item with a bbox: zoom there (helper A refines this into the full row).
        if resp.clicked()
            && let Some(item) = row["items"].as_array().and_then(|items| items.iter().find(|i| i["bbox"].is_array()))
            && let (Some(p), Some(bb)) = (item["page"].as_u64(), bbox(&item["bbox"]))
        {
            view.prepress.zoom_to = Some(((p as usize).saturating_sub(1), bb));
        }
    }
    let _ = t;
}
