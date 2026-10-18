//! Prepress ▸ Checks: the rows from `/api/checks` (`#`, status, check, summary). A row with items
//! expands to list them; an item with a bbox zooms the page to it (a row with exactly one such item
//! zooms on its own click). Nothing is drawn on the page here: the app's own zoom is the window.

use std::collections::HashSet;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, vec2};
use serde_json::Value;

use crate::theme::{self, Tokens};
use crate::{DocView, icons, widgets};

use super::{Load, bbox};

/// Which rows are expanded (by 0-based row index). Lives on `prepress::State`, so per document.
#[derive(Default)]
pub struct Ui {
    open: HashSet<usize>,
}

#[derive(Debug, PartialEq)]
pub struct Item {
    /// 0-based page.
    pub page: usize,
    /// View-space points (y down); `None` = the whole page, nothing to zoom to.
    pub bbox: Option<[f32; 4]>,
    pub note: String,
}

#[derive(Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub status: String,
    pub summary: String,
    pub items: Vec<Item>,
}

impl Row {
    /// The zoom target when the row has exactly one item with a bbox.
    fn only_zoom(&self) -> Option<(usize, [f32; 4])> {
        let mut z = self.items.iter().filter_map(|i| Some((i.page, i.bbox?)));
        let first = z.next()?;
        z.next().is_none().then_some(first)
    }
}

/// The server's `/api/checks` JSON as rows; pages become 0-based.
pub fn rows_from(json: &Value) -> Vec<Row> {
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    json["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Row {
            name: s(&c["check"]),
            status: s(&c["status"]),
            summary: s(&c["summary"]),
            items: c["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| Item { page: (i["page"].as_u64().unwrap_or(1) as usize).saturating_sub(1), bbox: bbox(&i["bbox"]), note: s(&i["note"]) })
                .collect(),
        })
        .collect()
}

/// Kit colours: good / warn / bad, light or dark; anything else (SKIPPED, ERROR) is muted.
fn status_color(t: &Tokens, status: &str) -> Option<Color32> {
    let d = t.dark();
    Some(match status {
        "PASS" => {
            if d {
                Color32::from_rgb(0x34, 0xD3, 0x99)
            } else {
                Color32::from_rgb(0x0C, 0x87, 0x5D)
            }
        }
        "WARN" => {
            if d {
                Color32::from_rgb(0xFB, 0xBF, 0x24)
            } else {
                Color32::from_rgb(0xAA, 0x67, 0x00)
            }
        }
        "FAIL" => {
            if d {
                Color32::from_rgb(0xF8, 0x71, 0x71)
            } else {
                Color32::from_rgb(0xD5, 0x41, 0x41)
            }
        }
        _ => return None,
    })
}

/// A status pill: tinted background, coloured text, a minimum width so the names line up.
fn chip(ui: &mut egui::Ui, t: &Tokens, status: &str, min_w: f32) {
    let font = theme::semibold(9.5);
    let (fg, bg) = match status_color(t, status) {
        Some(c) => (c, c.gamma_multiply(0.16)),
        None => (t.text_muted, t.hover),
    };
    let w = ui.fonts_mut(|f| f.layout_no_wrap(status.to_string(), font.clone(), fg).size().x) + 12.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w.max(min_w), 16.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, status, font, fg);
}

fn zoom(view: &mut DocView, page: usize, bb: [f32; 4]) {
    if view.current != page {
        view.go_to_page(page);
    }
    view.prepress.zoom_to = Some((page, bb));
}

/// A clickable block: a hover/selected wash behind `add`'s content, the whole block one target.
fn click_block(ui: &mut egui::Ui, t: &Tokens, id: egui::Id, selected: bool, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let bg = ui.painter().add(egui::Shape::Noop);
    // The block spans the whole row, so the chevron / zoom icon lands in the strip reserved for it.
    let right = ui.max_rect().right();
    let mut rect = ui.horizontal_top(add).response.rect;
    rect.max.x = right;
    let rect = rect.expand2(vec2(2.0, 3.0));
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().set(bg, egui::Shape::rect_filled(rect, CornerRadius::same(6), fill));
    ui.add_space(4.0);
    resp
}

fn label(text: impl Into<String>, font: egui::FontId, color: Color32) -> egui::Label {
    egui::Label::new(egui::RichText::new(text).font(font).color(color)).selectable(false)
}

pub(super) fn panel(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, json: &Value) {
    let rows = rows_from(json);
    let verdict = json["verdict"].as_str().unwrap_or("");
    ui.horizontal(|ui| {
        chip(ui, t, verdict, 0.0);
        ui.add(label(format!("{} checks", rows.len()), theme::medium(12.0), t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::pill_button(ui, "Re-run", false).on_hover_text("Ask the viewer server again (it re-reads the PDF if it changed)").clicked()
                && let Ok(mut l) = view.prepress.checks.lock()
            {
                *l = Load::Idle;
            }
        });
    });
    ui.add_space(6.0);
    let (num_w, chip_w) = (18.0, 54.0);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(num_w, 14.0), label("#", theme::semibold(10.0), t.text_faint));
        ui.add_sized(vec2(chip_w, 14.0), label("Status", theme::semibold(10.0), t.text_faint));
        ui.add(label("Check", theme::semibold(10.0), t.text_faint));
    });
    ui.separator();
    for (n, row) in rows.iter().enumerate() {
        let open = view.prepress.checks_ui.open.contains(&n);
        let has_items = !row.items.is_empty();
        let resp = click_block(ui, t, ui.id().with(("prepress-check", n)), open, |ui| {
            ui.add_sized(vec2(num_w, 16.0), label((n + 1).to_string(), theme::regular(11.0), t.text_faint));
            chip(ui, t, &row.status, chip_w);
            let w = ui.available_width() - if has_items { 18.0 } else { 0.0 };
            ui.allocate_ui(vec2(w, 0.0), |ui| {
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.add(label(&row.name, theme::medium(12.5), t.text).wrap());
                    let mut summary = row.summary.clone();
                    if has_items {
                        let k = row.items.len();
                        summary.push_str(&format!(" · {k} item{}", if k == 1 { "" } else { "s" }));
                    }
                    ui.add(label(summary, theme::regular(11.0), t.text_muted).wrap());
                });
            });
        });
        if has_items {
            let r = Rect::from_center_size(egui::pos2(resp.rect.right() - 10.0, resp.rect.top() + 11.0), vec2(14.0, 14.0));
            icons::paint(ui, r, if open { "chevron-down" } else { "chevron-right" }, 14.0, t.text_muted);
        }
        if resp.clicked() && has_items {
            if !view.prepress.checks_ui.open.remove(&n) {
                view.prepress.checks_ui.open.insert(n);
            }
            if let Some((p, bb)) = row.only_zoom() {
                zoom(view, p, bb);
            }
        }
        if !open {
            continue;
        }
        for (k, item) in row.items.iter().enumerate() {
            let note = item.note.strip_prefix(&format!("p{}: ", item.page + 1)).unwrap_or(&item.note);
            let add = |ui: &mut egui::Ui| {
                ui.add_space(num_w + 8.0);
                ui.add_sized(vec2(26.0, 16.0), label(format!("p{}", item.page + 1), theme::semibold(11.0), t.text));
                let w = ui.available_width() - 22.0;
                ui.allocate_ui(vec2(w, 0.0), |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.add(label(note, theme::regular(11.0), t.text_muted).wrap());
                        if item.bbox.is_none() {
                            ui.add(label("whole page · nothing to zoom to", theme::regular(10.5), t.text_faint));
                        }
                    });
                });
            };
            match item.bbox {
                Some(bb) => {
                    let resp = click_block(ui, t, ui.id().with(("prepress-check-item", n, k)), false, add).on_hover_text("Zoom to this");
                    let r = Rect::from_center_size(egui::pos2(resp.rect.right() - 10.0, resp.rect.top() + 11.0), vec2(14.0, 14.0));
                    icons::paint(ui, r, "zoom-in", 14.0, t.accent);
                    if resp.clicked() {
                        zoom(view, item.page, bb);
                    }
                }
                None => {
                    ui.horizontal_top(add);
                    ui.add_space(4.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_carry_status_items_and_zero_based_zoom_targets() {
        let json = serde_json::json!({"verdict": "FAIL", "checks": [
            {"check": "Minimum text size", "status": "WARN", "summary": "2 small", "details": [],
             "items": [{"page": 1, "bbox": [1, 2, 3, 4], "note": "p1: a"}, {"page": 2, "bbox": [5, 6, 7, 8], "note": "p2: b"}]},
            {"check": "Bleed & trim box", "status": "FAIL", "summary": "no bleed", "details": [],
             "items": [{"page": 1, "bbox": null, "note": "p1: whole"}]},
            {"check": "Die fit", "status": "SKIPPED", "summary": "no die", "details": [], "items": []}
        ]});
        let rows = rows_from(&json);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(), ["WARN", "FAIL", "SKIPPED"]);
        let pairs: Vec<_> = rows[0].items.iter().map(|i| (i.page, i.bbox)).collect();
        assert_eq!(pairs, [(0, Some([1.0, 2.0, 3.0, 4.0])), (1, Some([5.0, 6.0, 7.0, 8.0]))]);
        assert_eq!(rows[0].only_zoom(), None, "two targets: the row expands, an item zooms");
        assert_eq!(rows[1].items[0].bbox, None);
        assert_eq!(rows[1].only_zoom(), None);
        assert!(rows[2].items.is_empty());
    }
}
