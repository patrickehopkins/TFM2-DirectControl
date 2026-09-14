//! Temporary minimap hit-test + coordinate diagnostic.
//!
//! Runtime UI-tree discovery proved too brittle for the first minimap pass: the original recursive
//! search caused stalls, and the bounded replacement still failed to recognize real minimap clicks.
//! For this diagnostic build we deliberately remove UI-tree discovery entirely and use the minimap's
//! stable lower-right layout proportions observed in the live match UI. Every RMB routed through this
//! helper is logged to `%TEMP%\tfm2_direct_control_minimap.log` so one test can tell us whether the
//! failure is hit detection or the later simulation command handoff.

use std::{
    fs::{self, OpenOptions},
    io::Write,
};

use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

// Broad temporary hit gate. This intentionally extends beyond the visible minimap so a real minimap
// click cannot miss merely because our calibration is a few pixels off. It is diagnostic, not final.
const HIT_LEFT_FRAC: f32 = 0.77;
const HIT_TOP_FRAC: f32 = 0.60;

// Mapping rectangle measured from the live lower-right minimap in the supplied 16:9 screenshots.
// Keep hit detection broad, but map against the tighter visible minimap bounds.
const MAP_LEFT_FRAC: f32 = 0.815;
const MAP_TOP_FRAC: f32 = 0.685;
const MAP_RIGHT_FRAC: f32 = 0.995;
const MAP_BOTTOM_FRAC: f32 = 0.990;

fn ui_size(ctx: &StableClient<'_>) -> (f32, f32) {
    ctx.draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H))
}

fn diagnostic_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tfm2_direct_control_minimap.log")
}

fn log_attempt(
    ui_x: f32,
    ui_y: f32,
    ui_w: f32,
    ui_h: f32,
    hit: bool,
    mapped: Option<(u64, u64)>,
) {
    let path = diagnostic_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let mapped_text = mapped
            .map(|(x, y)| format!("{x},{y}"))
            .unwrap_or_else(|| "none".to_owned());
        let _ = writeln!(
            file,
            "ui=({ui_x:.1},{ui_y:.1}) size=({ui_w:.1},{ui_h:.1}) lower_right_hit={hit} mapped={mapped_text}"
        );
    }
}

pub fn reset() {
    // Start each match with a clean diagnostic file so the last few lines always describe the
    // current test rather than an earlier build/session.
    let _ = fs::remove_file(diagnostic_log_path());
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
        log_attempt(ui_x, ui_y, ui_w, ui_h, false, None);
        return None;
    }

    let hit = ui_x >= ui_w * HIT_LEFT_FRAC
        && ui_x < ui_w
        && ui_y >= ui_h * HIT_TOP_FRAC
        && ui_y < ui_h;
    if !hit {
        log_attempt(ui_x, ui_y, ui_w, ui_h, false, None);
        return None;
    }

    let left = ui_w * MAP_LEFT_FRAC;
    let top = ui_h * MAP_TOP_FRAC;
    let right = ui_w * MAP_RIGHT_FRAC;
    let bottom = ui_h * MAP_BOTTOM_FRAC;
    let width = (right - left).max(1.0);
    let height = (bottom - top).max(1.0);

    // Broad hit gate means a click just outside our tighter measured rectangle can still be accepted;
    // clamp it to the nearest minimap edge rather than falling back into camera-relative projection.
    let nx = ((ui_x - left) / width).clamp(0.0, 1.0);
    let ny = ((ui_y - top) / height).clamp(0.0, 1.0);
    let mapped = (
        (nx * DEFAULT_MAP_MAX_SIM).round() as u64,
        (ny * DEFAULT_MAP_MAX_SIM).round() as u64,
    );

    log_attempt(ui_x, ui_y, ui_w, ui_h, true, Some(mapped));
    Some(mapped)
}
