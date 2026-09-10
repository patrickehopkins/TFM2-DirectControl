//! Read-only pause-state discovery from the live client UI.
//!
//! The stable API does not expose a dedicated pause bit. Stage 4C also proved that the game's
//! visible pause menu is not represented as a simple `ui_text("Resume")` label: the actual menu
//! shows "Return to Game" and the label-only scan missed it entirely.
//!
//! This probe therefore combines three read-only signals:
//! 1. visible text/state containing Return-to-Game / Resume wording;
//! 2. a large, central visible UI node whose semantic path contains "pause";
//! 3. a conservative fallback: after the visible match clock has changed at least once, if that
//!    clock then remains unchanged for >1.25 s, treat presentation as paused/stalled.
//!
//! The clock fallback is intentionally secondary. Direct-control mode currently targets 1x play;
//! variable live playback rates are deferred and can replace this heuristic later.

use std::sync::Mutex;

use mod_api_stable::StableClient;

const SCAN_INTERVAL_MS: u64 = 100;
const MAX_UI_NODES: usize = 2_000;
const CLOCK_STALL_MS: u64 = 1_250;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

#[derive(Debug, Clone)]
pub struct PauseUiSnapshot {
    pub paused: bool,
    pub scanned_nodes: usize,
    pub marker: Option<String>,
}

#[derive(Debug)]
struct PauseUiState {
    last_scan_ms: u64,
    paused: bool,
    scanned_nodes: usize,
    marker: Option<String>,
    last_clock: Option<String>,
    last_clock_change_ms: u64,
    clock_has_advanced: bool,
}

impl PauseUiState {
    const fn empty() -> Self {
        Self {
            last_scan_ms: 0,
            paused: false,
            scanned_nodes: 0,
            marker: None,
            last_clock: None,
            last_clock_change_ms: 0,
            clock_has_advanced: false,
        }
    }

    fn snapshot(&self) -> PauseUiSnapshot {
        PauseUiSnapshot {
            paused: self.paused,
            scanned_nodes: self.scanned_nodes,
            marker: self.marker.clone(),
        }
    }
}

static STATE: Mutex<PauseUiState> = Mutex::new(PauseUiState::empty());

pub fn reset() {
    if let Ok(mut state) = STATE.lock() {
        *state = PauseUiState::empty();
    }
}

pub fn update(ctx: &StableClient<'_>, interactive_match: bool) -> PauseUiSnapshot {
    let now_ms = unsafe { GetTickCount64() };
    let mut state = match STATE.lock() {
        Ok(state) => state,
        Err(_) => {
            return PauseUiSnapshot {
                paused: false,
                scanned_nodes: 0,
                marker: Some("pause-state mutex poisoned".to_owned()),
            }
        }
    };

    if !interactive_match {
        state.last_scan_ms = 0;
        state.paused = false;
        state.scanned_nodes = 0;
        state.marker = None;
        state.last_clock = None;
        state.last_clock_change_ms = 0;
        state.clock_has_advanced = false;
        return state.snapshot();
    }

    update_clock_state(ctx, now_ms, &mut state);

    if state.last_scan_ms != 0 && now_ms.saturating_sub(state.last_scan_ms) < SCAN_INTERVAL_MS {
        return state.snapshot();
    }
    state.last_scan_ms = now_ms;

    let (ui_paused, scanned_nodes, ui_marker) = scan_visible_pause_ui(ctx);
    let clock_stalled = state.clock_has_advanced
        && state.last_clock_change_ms != 0
        && now_ms.saturating_sub(state.last_clock_change_ms) >= CLOCK_STALL_MS;

    state.scanned_nodes = scanned_nodes;
    state.paused = ui_paused || clock_stalled;
    state.marker = if ui_paused {
        ui_marker
    } else if clock_stalled {
        Some(format!(
            "clock stalled {} ms at {}",
            now_ms.saturating_sub(state.last_clock_change_ms),
            state.last_clock.as_deref().unwrap_or("--:--")
        ))
    } else {
        ui_marker
    };

    state.snapshot()
}

fn update_clock_state(ctx: &StableClient<'_>, now_ms: u64, state: &mut PauseUiState) {
    let current = ctx.ui_text("ingame.header.game_time.value");
    let Some(current) = current else {
        return;
    };

    let current = current.trim().to_owned();
    match state.last_clock.as_deref() {
        None => {
            state.last_clock = Some(current);
            state.last_clock_change_ms = now_ms;
        }
        Some(previous) if previous != current => {
            state.last_clock = Some(current);
            state.last_clock_change_ms = now_ms;
            state.clock_has_advanced = true;
        }
        Some(_) => {}
    }
}

fn scan_visible_pause_ui(ctx: &StableClient<'_>) -> (bool, usize, Option<String>) {
    let mut stack = vec![String::new()];
    let mut scanned = 0usize;
    let mut best_hint: Option<String> = None;
    let (ui_w, ui_h) = ctx
        .draw_map_size("UI")
        .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));

    while let Some(parent) = stack.pop() {
        if scanned >= MAX_UI_NODES {
            break;
        }

        for child in ctx.ui_child_names(&parent) {
            if scanned >= MAX_UI_NODES {
                break;
            }

            let path = if parent.is_empty() {
                child
            } else {
                format!("{parent}.{child}")
            };
            scanned += 1;

            if matches!(ctx.ui_visible(&path), Some(false)) {
                continue;
            }

            let path_lower = path.to_ascii_lowercase();

            if let Some(text) = ctx.ui_text(&path) {
                let normalized = text.trim().to_ascii_lowercase();
                let is_return = matches!(
                    normalized.as_str(),
                    "return to game" | "resume" | "resume game" | "unpause"
                );
                if is_return {
                    return (
                        true,
                        scanned,
                        Some(format!("{} @ {} [text]", text.trim(), path)),
                    );
                }
                if (normalized == "pause" || normalized == "paused") && best_hint.is_none() {
                    best_hint = Some(format!("{} @ {} [text hint]", text.trim(), path));
                }
            }

            if let Some(state_json) = ctx.ui_state_json(&path) {
                let lower = state_json.to_ascii_lowercase();
                if lower.contains("return to game")
                    || lower.contains("resume game")
                    || lower.contains("\"resume\"")
                {
                    return (
                        true,
                        scanned,
                        Some(format!("{} [state]", path)),
                    );
                }
            }

            // The persistent right-side Pause button is small. The actual pause dialog is large
            // and central. If semantic ids expose "pause", use geometry to distinguish the two.
            if path_lower.contains("pause") {
                if let Some((x, y, w, h)) = ctx.ui_node_rect(&path) {
                    let cx = x + w * 0.5;
                    let cy = y + h * 0.5;
                    let large = w >= 260.0 && h >= 160.0;
                    let central = cx >= ui_w * 0.20
                        && cx <= ui_w * 0.80
                        && cy >= ui_h * 0.15
                        && cy <= ui_h * 0.85;
                    if large && central {
                        return (
                            true,
                            scanned,
                            Some(format!(
                                "{} [pause-path {:.0}x{:.0} @ {:.0},{:.0}]",
                                path, w, h, x, y
                            )),
                        );
                    }
                    if best_hint.is_none() {
                        best_hint = Some(format!(
                            "{} [pause-path hint {:.0}x{:.0} @ {:.0},{:.0}]",
                            path, w, h, x, y
                        ));
                    }
                } else if best_hint.is_none() {
                    best_hint = Some(format!("{} [pause-path hint]", path));
                }
            }

            stack.push(path);
        }
    }

    (false, scanned, best_hint)
}
