//! Runtime minimap discovery and UI->simulation coordinate mapping.
//!
//! The stable client API exposes the live UI tree and computed node rectangles, so we do not need
//! to hard-code a 1920x1080 minimap box. We discover a plausible minimap node by path/runner name,
//! cache that path for the current match, and convert clicks inside its live contents rectangle into
//! the default 960000x960000 simulation map.

use std::sync::Mutex;

use mod_api_stable::StableClient;

const DEFAULT_MAP_MAX_SIM: f32 = 960_000.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const MAX_SCAN_DEPTH: usize = 10;
const MAX_SCAN_NODES: usize = 2_048;

static MINIMAP_PATH: Mutex<Option<String>> = Mutex::new(None);

pub fn reset() {
    if let Ok(mut cached) = MINIMAP_PATH.lock() {
        *cached = None;
    }
}

fn usable_rect(rect: (f32, f32, f32, f32)) -> bool {
    let (_, _, w, h) = rect;
    w.is_finite() && h.is_finite() && w >= 48.0 && h >= 48.0 && w <= 640.0 && h <= 640.0
}

fn rect_for(ctx: &StableClient<'_>, path: &str) -> Option<(f32, f32, f32, f32)> {
    ctx.ui_contents_rect(path)
        .filter(|rect| usable_rect(*rect))
        .or_else(|| ctx.ui_node_rect(path).filter(|rect| usable_rect(*rect)))
}

fn name_score(path: &str, runner: &str) -> i32 {
    let path = path.to_ascii_lowercase();
    let runner = runner.to_ascii_lowercase();
    let joined = format!("{path} {runner}");

    if joined.contains("minimap") || joined.contains("mini_map") || joined.contains("mini-map") {
        200
    } else if joined.contains("mini") && joined.contains("map") {
        160
    } else if joined.contains("map") {
        40
    } else {
        0
    }
}

fn rect_score(
    rect: (f32, f32, f32, f32),
    ui_size: (f32, f32),
) -> i32 {
    let (x, y, w, h) = rect;
    let (ui_w, ui_h) = ui_size;
    if !usable_rect(rect) || ui_w <= 0.0 || ui_h <= 0.0 {
        return -10_000;
    }

    let aspect = w / h;
    let mut score = 0;
    if (0.75..=1.33).contains(&aspect) {
        score += 35;
    }
    if x + w * 0.5 > ui_w * 0.5 {
        score += 15;
    }
    if y + h * 0.5 > ui_h * 0.5 {
        score += 15;
    }
    if x + w * 0.5 > ui_w * 0.65 && y + h * 0.5 > ui_h * 0.65 {
        score += 20;
    }
    score
}

fn discover(ctx: &StableClient<'_>) -> Option<String> {
    let ui_size = ctx
        .draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));
    let mut stack = vec![(String::new(), 0usize)];
    let mut visited = 0usize;
    let mut best: Option<(i32, f32, String)> = None;

    while let Some((parent, depth)) = stack.pop() {
        if depth > MAX_SCAN_DEPTH || visited >= MAX_SCAN_NODES {
            continue;
        }

        for child in ctx.ui_child_names(&parent) {
            if visited >= MAX_SCAN_NODES {
                break;
            }
            visited += 1;

            let path = if parent.is_empty() {
                child
            } else {
                format!("{parent}.{child}")
            };

            if depth < MAX_SCAN_DEPTH {
                stack.push((path.clone(), depth + 1));
            }

            if matches!(ctx.ui_visible(&path), Some(false)) {
                continue;
            }

            let runner = ctx.ui_runner_name(&path).unwrap_or_default();
            let semantic = name_score(&path, &runner);
            if semantic == 0 {
                continue;
            }

            let Some(rect) = rect_for(ctx, &path) else {
                continue;
            };
            let score = semantic + rect_score(rect, ui_size);
            let area = rect.2 * rect.3;

            match &best {
                Some((best_score, best_area, _))
                    if score < *best_score || (score == *best_score && area <= *best_area) => {}
                _ => best = Some((score, area, path)),
            }
        }
    }

    // A generic small "map" widget is only accepted when its geometry strongly resembles the
    // bottom-right minimap. Explicit mini+map naming is accepted with much less ambiguity.
    best.and_then(|(score, _, path)| (score >= 100).then_some(path))
}

fn cached_path(ctx: &StableClient<'_>) -> Option<String> {
    if let Ok(cached) = MINIMAP_PATH.lock() {
        if let Some(path) = cached.as_ref() {
            if rect_for(ctx, path).is_some() && !matches!(ctx.ui_visible(path), Some(false)) {
                return Some(path.clone());
            }
        }
    }

    let discovered = discover(ctx)?;
    if let Ok(mut cached) = MINIMAP_PATH.lock() {
        *cached = Some(discovered.clone());
    }
    Some(discovered)
}

pub fn cursor_to_sim(
    ctx: &StableClient<'_>,
    ui_x: f32,
    ui_y: f32,
) -> Option<(u64, u64)> {
    let path = cached_path(ctx)?;
    let (x, y, w, h) = rect_for(ctx, &path)?;

    if ui_x < x || ui_y < y || ui_x >= x + w || ui_y >= y + h {
        return None;
    }

    let nx = ((ui_x - x) / w).clamp(0.0, 1.0);
    let ny = ((ui_y - y) / h).clamp(0.0, 1.0);
    Some((
        (nx * DEFAULT_MAP_MAX_SIM).round() as u64,
        (ny * DEFAULT_MAP_MAX_SIM).round() as u64,
    ))
}
