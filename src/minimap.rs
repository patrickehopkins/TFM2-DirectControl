//! Runtime minimap discovery and UI->simulation coordinate mapping.
//!
//! The first implementation recursively searched a large live UI tree from the RMB hot-path. When
//! the actual minimap node was not recognized, that made every click hitch and then fall through to
//! camera-relative battlefield projection. This version bounds discovery work and always provides a
//! camera-independent bottom-right fallback rectangle while discovery learns the exact live widget.

use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const MAX_SCAN_DEPTH: usize = 12;
const MAX_SCAN_NODES: usize = 4_096;
const SCAN_BUDGET_PER_CLICK: usize = 24;
const EARLY_ACCEPT_SCORE: i32 = 245;
const FINAL_ACCEPT_SCORE: i32 = 180;

type UiRect = (f32, f32, f32, f32);

#[derive(Debug, Clone)]
struct Candidate {
    score: i32,
    area: f32,
    path: String,
    rect: UiRect,
}

#[derive(Debug, Default)]
struct DiscoveryState {
    started: bool,
    complete: bool,
    visited: usize,
    queue: VecDeque<(String, usize)>,
    best: Option<Candidate>,
    cached_path: Option<String>,
}

static STATE: OnceLock<Mutex<DiscoveryState>> = OnceLock::new();

fn state() -> &'static Mutex<DiscoveryState> {
    STATE.get_or_init(|| Mutex::new(DiscoveryState::default()))
}

pub fn reset() {
    if let Ok(mut state) = state().lock() {
        *state = DiscoveryState::default();
    }
}

fn ui_size(ctx: &StableClient<'_>) -> (f32, f32) {
    ctx.draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H))
}

fn usable_rect(rect: UiRect) -> bool {
    let (_, _, w, h) = rect;
    w.is_finite()
        && h.is_finite()
        && w >= 180.0
        && h >= 180.0
        && w <= 560.0
        && h <= 560.0
}

fn rect_for(ctx: &StableClient<'_>, path: &str) -> Option<UiRect> {
    ctx.ui_contents_rect(path)
        .filter(|rect| usable_rect(*rect))
        .or_else(|| ctx.ui_node_rect(path).filter(|rect| usable_rect(*rect)))
}

fn fallback_rect(ctx: &StableClient<'_>) -> UiRect {
    let (ui_w, ui_h) = ui_size(ctx);
    // TFM2 anchors the square minimap at the lower-right. Express the fallback as proportions rather
    // than 1920x1080 pixels so window size/resolution does not matter. This is intentionally only a
    // fallback: live widget discovery supersedes it as soon as a trustworthy rectangle is found.
    let side = ui_h * 0.30;
    let right_margin = ui_w * 0.010;
    let bottom_margin = ui_h * 0.012;
    (
        (ui_w - side - right_margin).max(0.0),
        (ui_h - side - bottom_margin).max(0.0),
        side,
        side,
    )
}

fn semantic_score(path: &str, runner: &str) -> i32 {
    let joined = format!("{} {}", path.to_ascii_lowercase(), runner.to_ascii_lowercase());
    if joined.contains("minimap") || joined.contains("mini_map") || joined.contains("mini-map") {
        320
    } else if joined.contains("mini") && joined.contains("map") {
        260
    } else if joined.contains("map") {
        30
    } else {
        0
    }
}

fn geometry_score(rect: UiRect, ui_size: (f32, f32)) -> i32 {
    let (x, y, w, h) = rect;
    let (ui_w, ui_h) = ui_size;
    if !usable_rect(rect) || ui_w <= 0.0 || ui_h <= 0.0 {
        return -10_000;
    }

    let center_x = x + w * 0.5;
    let center_y = y + h * 0.5;
    if center_x < ui_w * 0.68 || center_y < ui_h * 0.52 {
        return -10_000;
    }

    let aspect = w / h;
    let mut score = if (0.82..=1.22).contains(&aspect) {
        80
    } else if (0.70..=1.40).contains(&aspect) {
        35
    } else {
        return -10_000;
    };

    if x + w >= ui_w * 0.92 {
        score += 45;
    }
    if y + h >= ui_h * 0.90 {
        score += 45;
    }
    if center_x >= ui_w * 0.78 && center_y >= ui_h * 0.68 {
        score += 35;
    }

    let side = (w + h) * 0.5;
    let expected = ui_h * 0.30;
    let relative_error = ((side - expected).abs() / expected.max(1.0)).min(1.0);
    score += (70.0 * (1.0 - relative_error)) as i32;
    if w >= 250.0 && h >= 250.0 {
        score += 25;
    }
    score
}

fn better(candidate: &Candidate, current: &Candidate) -> bool {
    candidate.score > current.score
        || (candidate.score == current.score && candidate.area > current.area)
}

fn scan_some(ctx: &StableClient<'_>) {
    let Ok(mut state) = state().lock() else {
        return;
    };

    if let Some(path) = state.cached_path.clone() {
        if !matches!(ctx.ui_visible(&path), Some(false)) && rect_for(ctx, &path).is_some() {
            return;
        }
        *state = DiscoveryState::default();
    }
    if state.complete {
        return;
    }

    if !state.started {
        state.started = true;
        for child in ctx.ui_child_names("") {
            state.queue.push_back((child, 0));
        }
    }

    let size = ui_size(ctx);
    for _ in 0..SCAN_BUDGET_PER_CLICK {
        if state.visited >= MAX_SCAN_NODES {
            state.complete = true;
            break;
        }
        let Some((path, depth)) = state.queue.pop_front() else {
            state.complete = true;
            break;
        };
        state.visited += 1;

        if depth < MAX_SCAN_DEPTH {
            for child in ctx.ui_child_names(&path) {
                state.queue.push_back((format!("{path}.{child}"), depth + 1));
            }
        }
        if matches!(ctx.ui_visible(&path), Some(false)) {
            continue;
        }
        let Some(rect) = rect_for(ctx, &path) else {
            continue;
        };
        let runner = ctx.ui_runner_name(&path).unwrap_or_default();
        let score = semantic_score(&path, &runner) + geometry_score(rect, size);
        if score < FINAL_ACCEPT_SCORE {
            continue;
        }

        let candidate = Candidate {
            score,
            area: rect.2 * rect.3,
            path,
            rect,
        };
        match state.best.as_ref() {
            Some(current) if !better(&candidate, current) => {}
            _ => state.best = Some(candidate),
        }

        if let Some(best) = state.best.clone() {
            if best.score >= EARLY_ACCEPT_SCORE {
                state.cached_path = Some(best.path);
                state.queue.clear();
                state.complete = true;
                return;
            }
        }
    }

    if state.complete {
        if let Some(best) = state.best.clone() {
            if best.score >= FINAL_ACCEPT_SCORE {
                state.cached_path = Some(best.path);
            }
        }
    }
}

fn discovered_rect(ctx: &StableClient<'_>) -> Option<UiRect> {
    let Ok(state) = state().lock() else {
        return None;
    };
    let path = state.cached_path.as_ref()?;
    if matches!(ctx.ui_visible(path), Some(false)) {
        return None;
    }
    rect_for(ctx, path)
}

fn point_inside(rect: UiRect, ui_x: f32, ui_y: f32) -> bool {
    let (x, y, w, h) = rect;
    ui_x >= x && ui_y >= y && ui_x < x + w && ui_y < y + h
}

pub fn cursor_to_sim(ctx: &StableClient<'_>, ui_x: f32, ui_y: f32) -> Option<(u64, u64)> {
    // Discovery is intentionally tiny and bounded. Even if no live node is ever found, the fallback
    // rectangle prevents minimap clicks from reaching camera-relative world projection.
    scan_some(ctx);

    let exact = discovered_rect(ctx);
    let fallback = fallback_rect(ctx);
    let rect = match exact {
        Some(rect) if point_inside(rect, ui_x, ui_y) => rect,
        _ if point_inside(fallback, ui_x, ui_y) => fallback,
        _ => return None,
    };

    let (x, y, w, h) = rect;
    let nx = ((ui_x - x) / w).clamp(0.0, 1.0);
    let ny = ((ui_y - y) / h).clamp(0.0, 1.0);
    Some((
        (nx * DEFAULT_MAP_MAX_SIM).round() as u64,
        (ny * DEFAULT_MAP_MAX_SIM).round() as u64,
    ))
}
