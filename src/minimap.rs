//! Camera-independent minimap RMB mapping.
//!
//! This pass deliberately removes live UI-tree discovery from the click path. The prior detector
//! first caused stalls, then could still miss or choose an unusable rectangle. TFM2's match minimap
//! is a stable lower-right square in the 1920x1080 logical UI, so use proportional geometry that
//! scales with the live UI map. Once this simple path is physically validated we can decide whether
//! exact node discovery is worth reintroducing at all.

use crate::control;
use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

// Calibrated from the live 16:9 match UI. Expressed as proportions so window size / render scale do
// not matter. The visible map square is roughly 29.6% of UI height, ~1% from the right edge and
// ~1.9% from the bottom edge.
const MINIMAP_SIDE_H_FRAC: f32 = 0.296;
const MINIMAP_RIGHT_W_FRAC: f32 = 0.010;
const MINIMAP_BOTTOM_H_FRAC: f32 = 0.019;
// Accept a small border around the visual map so clicks on its frame still count as minimap input.
const HIT_PAD_H_FRAC: f32 = 0.012;

type UiRect = (f32, f32, f32, f32);

pub fn reset() {}

fn ui_size(ctx: &StableClient<'_>) -> (f32, f32) {
    ctx.draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H))
}

fn minimap_rect(ctx: &StableClient<'_>) -> UiRect {
    let (ui_w, ui_h) = ui_size(ctx);
    let side = ui_h * MINIMAP_SIDE_H_FRAC;
    let right_margin = ui_w * MINIMAP_RIGHT_W_FRAC;
    let bottom_margin = ui_h * MINIMAP_BOTTOM_H_FRAC;
    (
        (ui_w - side - right_margin).max(0.0),
        (ui_h - side - bottom_margin).max(0.0),
        side.max(1.0),
        side.max(1.0),
    )
}

fn point_inside_padded(rect: UiRect, ui_x: f32, ui_y: f32, pad: f32) -> bool {
    let (x, y, w, h) = rect;
    ui_x >= x - pad && ui_y >= y - pad && ui_x < x + w + pad && ui_y < y + h + pad
}

pub fn cursor_to_sim(ctx: &StableClient<'_>, ui_x: f32, ui_y: f32) -> Option<(u64, u64)> {
    let (ui_w, ui_h) = ui_size(ctx);
    if !ui_x.is_finite() || !ui_y.is_finite() || ui_w <= 0.0 || ui_h <= 0.0 {
        return None;
    }

    let rect = minimap_rect(ctx);
    let pad = ui_h * HIT_PAD_H_FRAC;
    if !point_inside_padded(rect, ui_x, ui_y, pad) {
        return None;
    }

    let (x, y, w, h) = rect;
    let nx = ((ui_x - x) / w).clamp(0.0, 1.0);
    let ny = ((ui_y - y) / h).clamp(0.0, 1.0);
    let sim_x = (nx * DEFAULT_MAP_MAX_SIM).round() as u64;
    let sim_y = (ny * DEFAULT_MAP_MAX_SIM).round() as u64;

    // Dedicated move-only semantics: minimap RMB must replace Attack / Move / Return immediately and
    // must never be reinterpreted as an entity click merely because a unit happens to occupy the
    // corresponding world coordinate.
    control::publish_minimap_move_target(sim_x, sim_y);

    // lib.rs still consumes the returned coordinate through its legacy publisher. control.rs marks
    // that imminent request resolved so it cannot override the explicit minimap MoveTo above.
    Some((sim_x, sim_y))
}
