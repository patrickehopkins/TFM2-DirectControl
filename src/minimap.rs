//! Camera-independent minimap -> simulation coordinate mapping.
//!
//! TFM2 exposes the same square match minimap in two different placements:
//! - normal/full match view: minimap anchored at the lower-right;
//! - expanded Info UI view: minimap shifted left into the information panel.
//!
//! Only the minimap for the currently active match layout may be clickable. The active layout is
//! inferred from the live battlefield viewport (`ingame.center_log`), whose right edge moves far left
//! when Info UI opens. This avoids the earlier bug where both invisible minimap rectangles remained
//! active simultaneously.
//!
//! Physical testing confirmed that contextual RMB behavior on the minimap is desirable: ground
//! clicks become MoveTo and clicks over hostile markers can resolve to the same exact-target Attack
//! behavior as battlefield RMB. This module only recognizes the active minimap and converts that point
//! into simulation-space coordinates.

use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

// Measured from 2048x1152 captures, then expressed as fractions of the stable logical UI space.
const MINIMAP_SIDE_FRAC_OF_UI_H: f32 = 0.2960;

// Normal/full match view.
const FULL_LEFT_FRAC: f32 = 0.8237;
const FULL_TOP_FRAC: f32 = 0.6849;

// Expanded Info UI / split view.
const INFO_LEFT_FRAC: f32 = 0.5288;
const INFO_TOP_FRAC: f32 = 0.6762;

// In the observed layouts, the battlefield viewport right edge is around 0.80 UI width in full view
// and around 0.51 in Info UI. Keep the threshold comfortably between those states.
const INFO_VIEWPORT_RIGHT_EDGE_THRESHOLD: f32 = 0.68;

// Small forgiveness for minimap borders/scaling. Mapping remains clamped to the true square interior.
const HIT_PAD_LOGICAL_PX: f32 = 8.0;

type UiRect = (f32, f32, f32, f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchLayout {
    Full,
    Info,
}

fn ui_size(ctx: &StableClient<'_>) -> (f32, f32) {
    ctx.draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H))
}

pub fn reset() {
    // No cached discovery state. Kept so match lifecycle code can continue calling minimap::reset().
}

fn active_layout(ctx: &StableClient<'_>, ui_w: f32) -> Option<MatchLayout> {
    let (x, _y, w, _h) = ctx.ui_node_rect("ingame.center_log")?;
    if !x.is_finite() || !w.is_finite() || ui_w <= 0.0 {
        return None;
    }

    let right_edge = x + w;
    Some(if right_edge < ui_w * INFO_VIEWPORT_RIGHT_EDGE_THRESHOLD {
        MatchLayout::Info
    } else {
        MatchLayout::Full
    })
}

fn active_minimap_rect(ctx: &StableClient<'_>, ui_w: f32, ui_h: f32) -> Option<UiRect> {
    let side = ui_h * MINIMAP_SIDE_FRAC_OF_UI_H;
    match active_layout(ctx, ui_w)? {
        MatchLayout::Full => Some((
            ui_w * FULL_LEFT_FRAC,
            ui_h * FULL_TOP_FRAC,
            side,
            side,
        )),
        MatchLayout::Info => Some((
            ui_w * INFO_LEFT_FRAC,
            ui_h * INFO_TOP_FRAC,
            side,
            side,
        )),
    }
}

fn point_inside_with_pad(rect: UiRect, ui_x: f32, ui_y: f32, pad: f32) -> bool {
    let (x, y, w, h) = rect;
    ui_x >= x - pad
        && ui_y >= y - pad
        && ui_x < x + w + pad
        && ui_y < y + h + pad
}

fn map_point(rect: UiRect, ui_x: f32, ui_y: f32) -> (u64, u64) {
    let (x, y, w, h) = rect;
    let nx = ((ui_x - x) / w.max(1.0)).clamp(0.0, 1.0);
    let ny = ((ui_y - y) / h.max(1.0)).clamp(0.0, 1.0);
    (
        (nx * DEFAULT_MAP_MAX_SIM).round() as u64,
        (ny * DEFAULT_MAP_MAX_SIM).round() as u64,
    )
}

pub fn cursor_to_sim(ctx: &StableClient<'_>, ui_x: f32, ui_y: f32) -> Option<(u64, u64)> {
    let (ui_w, ui_h) = ui_size(ctx);
    if !ui_x.is_finite()
        || !ui_y.is_finite()
        || !ui_w.is_finite()
        || !ui_h.is_finite()
        || ui_w <= 0.0
        || ui_h <= 0.0
    {
        return None;
    }

    let rect = active_minimap_rect(ctx, ui_w, ui_h)?;
    let pad = HIT_PAD_LOGICAL_PX * (ui_h / UI_FALLBACK_H);
    point_inside_with_pad(rect, ui_x, ui_y, pad).then(|| map_point(rect, ui_x, ui_y))
}
