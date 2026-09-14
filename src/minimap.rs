//! Camera-independent minimap -> simulation coordinate mapping.
//!
//! TFM2 exposes the same square match minimap in two different placements:
//! - normal/full match view: minimap anchored at the lower-right;
//! - expanded Info UI view: minimap shifted left into the information panel.
//!
//! Physical testing confirmed that contextual RMB behavior on the normal minimap is desirable:
//! ground clicks become MoveTo and clicks over hostile markers can resolve to the same exact-target
//! Attack behavior as battlefield RMB. Keep that shared resolver; this module only recognizes which
//! minimap was clicked and converts that point into simulation-space coordinates.
//!
//! Do not scan the live UI tree from the click hot-path. Earlier discovery attempts caused stalls
//! when the expected node was not found. The stable UI draw space is 1920x1080 and both observed
//! minimaps retain the same geometry, so two proportional rectangles are sufficient and remain
//! independent of the OS window/client pixel size.

use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

// Measured from 2048x1152 captures, then expressed as fractions of the stable logical UI space.
// Both interiors are ~341x341 physical pixels there, i.e. ~320x320 in 1920x1080 logical UI units.
const MINIMAP_SIDE_FRAC_OF_UI_H: f32 = 0.2960;

// Normal/full match view.
const FULL_LEFT_FRAC: f32 = 0.8237;
const FULL_TOP_FRAC: f32 = 0.6849;

// Expanded Info UI / split view.
const INFO_LEFT_FRAC: f32 = 0.5288;
const INFO_TOP_FRAC: f32 = 0.6762;

// A tiny amount of forgiveness keeps edge clicks from missing due to borders/scaling. Mapping itself
// is still clamped to the true square interior, so padded clicks project to the nearest map edge.
const HIT_PAD_LOGICAL_PX: f32 = 8.0;

type UiRect = (f32, f32, f32, f32);

fn ui_size(ctx: &StableClient<'_>) -> (f32, f32) {
    ctx.draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H))
}

pub fn reset() {
    // No cached discovery state. Kept so match lifecycle code can continue calling minimap::reset().
}

fn minimap_rects(ui_w: f32, ui_h: f32) -> [UiRect; 2] {
    let side = ui_h * MINIMAP_SIDE_FRAC_OF_UI_H;
    [
        (ui_w * FULL_LEFT_FRAC, ui_h * FULL_TOP_FRAC, side, side),
        (ui_w * INFO_LEFT_FRAC, ui_h * INFO_TOP_FRAC, side, side),
    ]
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

    // Scale the small logical-edge forgiveness with the current stable UI dimensions.
    let pad = HIT_PAD_LOGICAL_PX * (ui_h / UI_FALLBACK_H);
    for rect in minimap_rects(ui_w, ui_h) {
        if point_inside_with_pad(rect, ui_x, ui_y, pad) {
            return Some(map_point(rect, ui_x, ui_y));
        }
    }

    None
}
