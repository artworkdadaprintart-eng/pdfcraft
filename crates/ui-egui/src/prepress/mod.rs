//! XpressFlow Prepress: the suite's label-press checks drawn inside the app.
//!
//! The data comes from the suite's viewer server (`preflight-prepress/viewer`, a stdlib HTTP server
//! that opens one PDF and serves this app at `/pdfcraft/`). Everything here is a same-origin fetch of
//! its JSON (`/api/checks`, `/api/separations?page=`, `/api/overprint?page=`, `/api/tac?page=`) and
//! its PNGs (`/img/<name>`), so the browser build is the real one; the desktop build says so and
//! stops. Four sections share one panel (Tools ▸ Prepress); each tool item opens its section.
//!
//! Coordinates: the server's `bbox` values are PyMuPDF page points (origin top-left, y down, after
//! the page's rotation), which is this app's *view space* for the page, so `PageXform::view_rect`
//! maps them straight to the screen. A PNG from the server covers the whole page and is painted over
//! it as a mesh that follows zoom, pan and rotation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, Rect};
use serde_json::Value;

use crate::canvas::PageXform;
use crate::theme::Tokens;
use crate::{DocView, PdfCraftApp, widgets};

/// The Tools column is widened to this when Prepress opens (one line per check needs the room).
pub const PANEL_WIDTH: f32 = 420.0;

pub mod checks;
pub mod overprint;
pub mod separations;
pub mod tac;
pub mod trap;

/// Where the viewer server answers. Same origin as the page this app is served from.
pub const API: &str = "/api";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Checks,
    Separations,
    Overprint,
    Tac,
    Trap,
}

impl Section {
    pub const ALL: [Self; 5] = [Self::Checks, Self::Separations, Self::Overprint, Self::Tac, Self::Trap];

    pub fn label(self) -> &'static str {
        match self {
            Self::Checks => "Checks",
            Self::Separations => "Separations",
            Self::Overprint => "Overprint",
            Self::Tac => "Ink coverage",
            Self::Trap => "Trap",
        }
    }

    /// The key prefix of this section's overlays (`Overlay::key`); empty for Checks, which draws none.
    pub fn overlay_prefix(self) -> &'static str {
        match self {
            Self::Checks => "",
            Self::Separations => "sep:",
            Self::Overprint => "overprint:",
            Self::Tac => "tac",
            Self::Trap => "trap:",
        }
    }

    /// `prepress.checks` and friends (the catalogue's commands).
    pub fn from_command(id: &str) -> Option<Self> {
        match id.strip_prefix("prepress.")? {
            "checks" => Some(Self::Checks),
            "separations" => Some(Self::Separations),
            "overprint" => Some(Self::Overprint),
            "tac" => Some(Self::Tac),
            "trap" => Some(Self::Trap),
            _ => None,
        }
    }
}

/// One fetch: not started, in flight, or finished either way.
#[derive(Clone, Debug, Default)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Ready(Value),
    Failed(String),
}

/// A fetch result shared with the future that fills it.
pub type Shared = Arc<Mutex<Load>>;
/// A PNG fetch in flight: empty until the viewer answers.
pub type PngSlot = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

/// A server PNG drawn over one page: a plate, the press view, the TAC heat map.
pub struct Overlay {
    /// Which section put it there, and what it is (`sep:Cyan`, `overprint:press`, `tac`).
    pub key: String,
    /// 0-based page.
    pub page: usize,
    pub tex: egui::TextureHandle,
    pub on: bool,
    /// Multiplied into the texture; `Color32::WHITE` shows it as is. Alpha here dims the whole overlay.
    pub tint: Color32,
}

/// Per-document state. Lives on [`DocView`], so each open PDF keeps its own.
#[derive(Default)]
pub struct State {
    pub section: Section,
    pub checks: Shared,
    pub checks_ui: checks::Ui,
    /// By 0-based page.
    pub separations: HashMap<usize, Shared>,
    pub overprint: HashMap<usize, Shared>,
    pub tac: HashMap<usize, Shared>,
    pub trap: HashMap<usize, Shared>,
    /// The last "Trap this file" answer (`/api/trap_save`): one per document.
    pub trap_save: Shared,
    /// `/api/info` (the named profiles and the editor link), fetched once per document.
    pub info: Shared,
    /// One-shot server actions (report, downsample, release, remap) by URL: Idle until clicked.
    pub actions: HashMap<String, Shared>,
    /// The last remap asked for from the Separations menu, so its answer stays on screen.
    pub remap_url: Option<String>,
    pub overlays: Vec<Overlay>,
    /// A check item to zoom to on the next paint: 0-based page, view-space bbox (pt, y down).
    pub zoom_to: Option<(usize, [f32; 4])>,
    /// PNGs being fetched, so a frame does not ask twice.
    pub pending_png: HashMap<String, PngSlot>,
}

impl State {
    /// The shared slot for a per-page section, created idle on first use.
    pub fn slot(&mut self, section: Section, page: usize) -> Shared {
        let map = match section {
            Section::Checks => return self.checks.clone(),
            Section::Separations => &mut self.separations,
            Section::Overprint => &mut self.overprint,
            Section::Tac => &mut self.tac,
            Section::Trap => &mut self.trap,
        };
        map.entry(page).or_default().clone()
    }

    pub fn overlay_mut(&mut self, key: &str, page: usize) -> Option<&mut Overlay> {
        self.overlays.iter_mut().find(|o| o.key == key && o.page == page)
    }

    /// Drop every overlay of one section (its key prefix), e.g. when its page changes.
    pub fn clear_overlays(&mut self, prefix: &str) {
        self.overlays.retain(|o| !o.key.starts_with(prefix));
    }
}

/// `prepress.<section>` from the Tools sidebar, the palette or automation.
pub(crate) fn command(app: &mut PdfCraftApp, id: &str) {
    app.select_tool("prepress");
    if let Some(section) = Section::from_command(id)
        && let Some((index, _)) = app.active_ids()
    {
        app.views[index].prepress.section = section;
    }
}

/// The Prepress panel body (below the shared panel header).
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some((index, _)) = app.active_ids() else {
        ui.label(egui::RichText::new("Open a PDF to run the prepress checks.").color(t.text_muted));
        return;
    };
    let ctx = ui.ctx().clone();
    let view = &mut app.views[index];
    let page = view.current;
    // `/api/info` (profiles, editor link, artwork-or-proof), once per document.
    let info = view.prepress.info.clone();
    ensure_json(&info, &format!("{API}/info"), &ctx);
    let info = match info.lock().map(|l| l.clone()).unwrap_or_default() {
        Load::Ready(i) => i,
        _ => Value::Null,
    };
    let proof = proof_kind(&info);
    proof_banner(ui, t, &info);
    ui.horizontal_wrapped(|ui| {
        for s in Section::ALL {
            if widgets::pill_button(ui, s.label(), view.prepress.section == s).clicked() {
                view.prepress.section = s;
            }
        }
    });
    ui.add_space(6.0);
    let section = view.prepress.section;
    let slot = view.prepress.slot(section, page);
    let url = match section {
        Section::Checks => checks::url(&view.prepress.checks_ui),
        Section::Separations => format!("{API}/separations?page={}", page + 1),
        Section::Overprint => format!("{API}/overprint?page={}", page + 1),
        Section::Tac => format!("{API}/tac?page={}", page + 1),
        Section::Trap => trap::url(page, trap::knobs(&ctx)),
    };
    ensure_json(&slot, &url, &ctx);
    let state = slot.lock().map(|l| l.clone()).unwrap_or_default();
    // The Outputs row is pinned to the bottom: the body gets the height above it (measured last frame).
    let outputs_id = ui.id().with("prepress-outputs-height");
    let outputs_h = ui.data(|d| d.get_temp::<f32>(outputs_id)).unwrap_or(44.0);
    match state {
        Load::Idle | Load::Loading => {
            ui.label(egui::RichText::new("Asking the viewer server…").color(t.text_muted));
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        Load::Failed(e) => {
            ui.colored_label(Color32::from_rgb(0xD5, 0x41, 0x41), e);
            if widgets::pill_button(ui, "Retry", false).clicked()
                && let Ok(mut l) = slot.lock()
            {
                *l = Load::Idle;
            }
        }
        Load::Ready(json) => {
            let body_h = (ui.available_height() - outputs_h).max(80.0);
            egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, false]).show(ui, |ui| match section {
                Section::Checks => checks::panel(ui, t, view, &json, &ctx, &info, proof == Some("approval_proof")),
                Section::Separations => separations::panel(ui, t, view, &json, &ctx),
                Section::Overprint => overprint::panel(ui, t, view, &json, &ctx),
                Section::Tac => tac::panel(ui, t, view, &json, &ctx),
                Section::Trap => trap::panel(ui, t, view, &json, &ctx),
            });
        }
    }
    let gap = ui.available_height() - outputs_h;
    if gap > 0.0 {
        ui.add_space(gap);
    }
    let h = ui.scope(|ui| outputs(ui, t, view, &ctx)).response.rect.height() + ui.spacing().item_spacing.y;
    ui.data_mut(|d| d.insert_temp(outputs_id, h));
}

/// `proof.kind` from `/api/info`: `Some("approval_proof")`, `Some("unknown")`, or `None` (artwork, or an
/// older server without the field).
pub fn proof_kind(info: &Value) -> Option<&str> {
    info["proof"]["kind"].as_str().filter(|k| matches!(*k, "approval_proof" | "unknown"))
}

/// A banner when the open file is an approval proof rather than the artwork; a quiet line when the server
/// could not tell. Hover shows the server's reason.
fn proof_banner(ui: &mut egui::Ui, t: &Tokens, info: &Value) {
    let why = |p: &Value| {
        let mut s = p["why"].as_str().unwrap_or("").to_string();
        if let Some(c) = p["confidence"].as_f64() {
            s.push_str(&format!(" (confidence {c:.2})"));
        }
        if let Some(by) = p["by"].as_str() {
            s.push_str(&format!(" - by {by}"));
        }
        s
    };
    match proof_kind(info) {
        Some("approval_proof") => {
            let c = checks::status_color(t, "FAIL").unwrap_or(t.text);
            egui::Frame::NONE.fill(c.gamma_multiply(0.16)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::same(10)).show(
                ui,
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new("This is an approval proof, not the artwork - open the artwork file")
                            .font(crate::theme::semibold(12.0))
                            .color(c),
                    )
                    .on_hover_text(why(&info["proof"]));
                },
            );
            ui.add_space(6.0);
        }
        Some(_) => {
            let c = checks::status_color(t, "WARN").unwrap_or(t.text_muted);
            ui.label(egui::RichText::new("Could not tell artwork from proof").font(crate::theme::regular(11.0)).color(c))
                .on_hover_text(why(&info["proof"]));
            ui.add_space(4.0);
        }
        None => {}
    }
}

/// Report PDF, Downsample and Release PDF/X-4: one row at the bottom of the panel, each written beside
/// the PDF on a click, never automatically; their answers show under the row.
fn outputs(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, ctx: &egui::Context) {
    let actions = [
        ("Report PDF", format!("{API}/report"), "<name>_report.pdf + _findings.csv beside the file"),
        ("Downsample", format!("{API}/downsample"), "Images above the profile's ppi resampled into <name>_light.pdf"),
        ("Release PDF/X-4", format!("{API}/release"), "Kura writes <name>_x4.pdf and re-checks it"),
    ];
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Outputs").font(crate::theme::semibold(10.0)).color(t.text_faint));
        for (label, url, hover) in &actions {
            let busy = view.prepress.actions.get(url).is_some_and(|s| s.lock().map(|l| matches!(*l, Load::Loading)).unwrap_or(false));
            if widgets::pill_button(ui, label, false).on_hover_text(*hover).clicked() && !busy {
                run_action(view, ctx, url);
            }
        }
    });
    for (_, url, _) in &actions {
        action_status(ui, t, view, ctx, url);
    }
}

// ---- painting on the page ----

/// Called once per visible page from the canvas, after comments. Draws this page's overlays, then
/// performs a pending zoom-to (the page's screen rectangle is only known here).
pub(crate) fn paint_page(painter: &egui::Painter, xf: &PageXform, page: usize, view: &mut DocView) {
    // Only the open section's overlays show (an opaque press view must not sit under the TAC heat map);
    // each section keeps its own on/off state for when it is opened again.
    let prefix = view.prepress.section.overlay_prefix();
    for o in view.prepress.overlays.iter().filter(|o| o.on && o.page == page && !prefix.is_empty() && o.key.starts_with(prefix)) {
        paint_tinted(painter, xf, o.tex.id(), o.tint);
    }
    if let Some((p, bb)) = view.prepress.zoom_to
        && p == page
    {
        view.prepress.zoom_to = None;
        let r = xf.view_rect(bb);
        // A hairline item still deserves a visible window around it.
        let pad = (r.width().max(r.height()) * 0.5).max(24.0);
        view.zoom_to_rect(Rect::from_center_size(r.center(), r.size() + egui::vec2(pad, pad)));
    }
}

/// [`PageXform::paint_image`] with a tint (that one is white-only).
fn paint_tinted(painter: &egui::Painter, xf: &PageXform, tex: egui::TextureId, tint: Color32) {
    let mut mesh = egui::Mesh::with_texture(tex);
    for (u, v) in [(0.0_f32, 0.0_f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        mesh.vertices.push(egui::epaint::Vertex { pos: xf.norm_to_screen(u, v), uv: egui::pos2(u, v), color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// `[x0, y0, x1, y1]` from the server, or `None` when it is null or malformed.
pub fn bbox(v: &Value) -> Option<[f32; 4]> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let mut out = [0.0; 4];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64()? as f32;
    }
    Some(out)
}

// ---- fetching ----

/// Start the fetch once; later frames see `Loading`, then `Ready`/`Failed`.
pub fn ensure_json(slot: &Shared, url: &str, ctx: &egui::Context) {
    let start = slot.lock().map(|l| matches!(*l, Load::Idle)).unwrap_or(false);
    if !start {
        return;
    }
    if let Ok(mut l) = slot.lock() {
        *l = Load::Loading;
    }
    fetch_json(url.to_string(), slot.clone(), ctx.clone());
}

/// Start (or restart) a one-shot server action; its answer lands in `State::actions[url]`.
pub(super) fn run_action(view: &mut DocView, ctx: &egui::Context, url: &str) {
    let slot = view.prepress.actions.entry(url.to_string()).or_default().clone();
    if let Ok(mut l) = slot.lock() {
        *l = Load::Idle;
    }
    ensure_json(&slot, url, ctx);
}

/// The answer of a one-shot action: "Working…", then "Written: <file>" (hover = the full
/// path, with MB before/after or the PDF/X-4 verdict when the server gives them) or the server's error.
pub(super) fn action_status(ui: &mut egui::Ui, t: &Tokens, view: &mut DocView, ctx: &egui::Context, url: &str) {
    let state = view.prepress.actions.get(url).and_then(|s| s.lock().ok().map(|l| l.clone())).unwrap_or_default();
    match &state {
        Load::Idle => {}
        Load::Loading => {
            ui.label(egui::RichText::new("Working…").color(t.text_muted));
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        Load::Ready(v) => {
            let path = v["path"].as_str().or(v["pdf"].as_str()).unwrap_or("");
            let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let mut extra = match (v["before_mb"].as_f64(), v["after_mb"].as_f64()) {
                (Some(b), Some(a)) => format!(" ({b:.1} → {a:.2} MB)"),
                _ => String::new(),
            };
            if let Some(c) = v["compliant"].as_bool() {
                extra.push_str(if c { " - PDF/X-4 compliant" } else { " - NOT PDF/X-4 compliant" });
            }
            if let Some(n) = v["note"].as_str() {
                extra.push_str(&format!(" - {n}"));
            }
            ui.label(egui::RichText::new(format!("Written: {name}{extra}")).color(Color32::from_rgb(0x0c, 0x87, 0x5d))).on_hover_text(path);
        }
        Load::Failed(e) => {
            ui.colored_label(Color32::from_rgb(0xd5, 0x41, 0x41), e);
        }
    }
}

/// `%`-escape the few characters an ink name can carry into a query value.
pub fn qs(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '&' => out.push_str("%26"),
            '%' => out.push_str("%25"),
            '+' => out.push_str("%2B"),
            '?' => out.push_str("%3F"),
            _ => out.push(c),
        }
    }
    out
}

/// Fetch a server PNG into a texture and add it as an overlay (idempotent per key+page). The
/// texture is uploaded on the frame the bytes arrive; until then the key is in `pending_png`.
pub fn ensure_overlay(view: &mut DocView, ctx: &egui::Context, key: &str, page: usize, url: &str, tint: Color32) {
    if view.prepress.overlay_mut(key, page).is_some() {
        return;
    }
    let id = format!("{key}@{page}");
    let slot = view.prepress.pending_png.entry(id.clone()).or_insert_with(|| {
        let slot = Arc::new(Mutex::new(None));
        fetch_bytes(url.to_string(), slot.clone(), ctx.clone());
        slot
    });
    let done = slot.lock().ok().and_then(|mut s| s.take());
    match done {
        None => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
        Some(Err(e)) => {
            view.prepress.pending_png.remove(&id);
            log::warn!("prepress overlay {key}: {e}");
        }
        Some(Ok(bytes)) => {
            view.prepress.pending_png.remove(&id);
            match decode_png(&bytes) {
                Ok(img) => {
                    let tex = ctx.load_texture(format!("prepress-{id}"), img, egui::TextureOptions::LINEAR);
                    view.prepress.overlays.push(Overlay { key: key.to_string(), page, tex, on: true, tint });
                }
                Err(e) => log::warn!("prepress overlay {key}: {e}"),
            }
        }
    }
}

/// RGBA8 `ColorImage` from PNG bytes (the `png` crate is already a workspace dependency).
pub fn decode_png(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buf[..info.buffer_size()].to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        (png::ColorType::Grayscale, png::BitDepth::Eight) => buf[..info.buffer_size()].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => {
            buf[..info.buffer_size()].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        (c, d) => return Err(format!("unsupported PNG {c:?}/{d:?}")),
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

#[cfg(target_arch = "wasm32")]
fn fetch_json(url: String, slot: Shared, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_text(&url).await.and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| format!("{url}: bad JSON: {e}")));
        if let Ok(mut l) = slot.lock() {
            *l = match result {
                Ok(v) if v["error"].is_string() => Load::Failed(v["error"].as_str().unwrap_or("").to_string()),
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e),
            };
        }
        ctx.request_repaint();
    });
}

#[cfg(target_arch = "wasm32")]
fn fetch_bytes(url: String, slot: PngSlot, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_response(&url).await;
        let result = match result {
            Ok(resp) => match resp.array_buffer() {
                Ok(p) => match wasm_bindgen_futures::JsFuture::from(p).await {
                    Ok(buf) => Ok(js_sys::Uint8Array::new(&buf).to_vec()),
                    Err(e) => Err(format!("{e:?}")),
                },
                Err(e) => Err(format!("{e:?}")),
            },
            Err(e) => Err(e),
        };
        if let Ok(mut s) = slot.lock() {
            *s = Some(result);
        }
        ctx.request_repaint();
    });
}

#[cfg(target_arch = "wasm32")]
async fn fetch_response(url: &str) -> Result<web_sys::Response, String> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or("no window")?;
    let resp = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| format!("{url}: {e:?}"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response")?;
    if !resp.ok() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    Ok(resp)
}

#[cfg(target_arch = "wasm32")]
async fn fetch_text(url: &str) -> Result<String, String> {
    let resp = fetch_response(url).await?;
    let text = wasm_bindgen_futures::JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    text.as_string().ok_or_else(|| "not text".to_string())
}

/// The desktop build has no viewer server beside it: say so instead of hanging on Loading.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_json(url: String, slot: Shared, ctx: egui::Context) {
    if let Ok(mut l) = slot.lock() {
        *l = Load::Failed(format!(
            "Prepress runs in the browser build served by the XpressFlow viewer (python -m viewer <file.pdf>); {url} is not reachable here."
        ));
    }
    ctx.request_repaint();
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_bytes(url: String, slot: PngSlot, ctx: egui::Context) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(Err(format!("{url}: no viewer server in the desktop build")));
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_map_to_their_commands() {
        for s in Section::ALL {
            let id = format!(
                "prepress.{}",
                match s {
                    Section::Checks => "checks",
                    Section::Separations => "separations",
                    Section::Overprint => "overprint",
                    Section::Tac => "tac",
                    Section::Trap => "trap",
                }
            );
            assert_eq!(Section::from_command(&id), Some(s));
        }
        assert_eq!(Section::from_command("prepress.nope"), None);
        assert_eq!(Section::from_command("measure.area"), None);
    }

    #[test]
    fn only_the_open_sections_overlays_match() {
        assert!("sep:Cyan".starts_with(Section::Separations.overlay_prefix()));
        assert!(!"sep:Cyan".starts_with(Section::Overprint.overlay_prefix()));
        assert!("overprint:press".starts_with(Section::Overprint.overlay_prefix()));
        assert!("tac".starts_with(Section::Tac.overlay_prefix()));
        assert!("trap:overlay".starts_with(Section::Trap.overlay_prefix()) && !"trap:overlay".starts_with(Section::Tac.overlay_prefix()));
        assert!(Section::Checks.overlay_prefix().is_empty(), "Checks paints no overlay");
    }

    #[test]
    fn proof_kind_reads_only_the_two_warning_kinds() {
        let info = |kind: Value| serde_json::json!({"preset": "x", "proof": {"kind": kind, "confidence": 0.9, "why": "w", "by": "rules"}});
        assert_eq!(proof_kind(&info("approval_proof".into())), Some("approval_proof"));
        assert_eq!(proof_kind(&info("unknown".into())), Some("unknown"));
        assert_eq!(proof_kind(&info("artwork".into())), None);
        assert_eq!(proof_kind(&serde_json::json!({"preset": "x"})), None, "an older server has no proof field");
        assert_eq!(proof_kind(&Value::Null), None, "/api/info not answered yet");
    }

    #[test]
    fn query_values_escape_ink_names() {
        assert_eq!(qs("PANTONE Rubine Red C"), "PANTONE%20Rubine%20Red%20C");
        assert_eq!(qs("A&B#1 +?%"), "A%26B%231%20%2B%3F%25");
        assert_eq!(qs("Cyan"), "Cyan");
    }

    #[test]
    fn bbox_reads_four_numbers_only() {
        assert_eq!(bbox(&serde_json::json!([1, 2.5, 3, 4])), Some([1.0, 2.5, 3.0, 4.0]));
        assert_eq!(bbox(&serde_json::json!(null)), None);
        assert_eq!(bbox(&serde_json::json!([1, 2, 3])), None);
        assert_eq!(bbox(&serde_json::json!([1, "x", 3, 4])), None);
    }

    #[test]
    fn png_decodes_to_rgba() {
        // 2×1 RGB PNG: red, blue.
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        }
        let img = decode_png(&out).unwrap();
        assert_eq!(img.size, [2, 1]);
        assert_eq!(img.pixels[0], Color32::from_rgba_unmultiplied(255, 0, 0, 255));
        assert_eq!(img.pixels[1], Color32::from_rgba_unmultiplied(0, 0, 255, 255));
    }

    #[test]
    fn slots_are_per_page_and_shared() {
        let mut s = State::default();
        let a = s.slot(Section::Tac, 0);
        let b = s.slot(Section::Tac, 0);
        let c = s.slot(Section::Tac, 1);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(Arc::ptr_eq(&s.slot(Section::Checks, 3), &s.checks));
    }
}
