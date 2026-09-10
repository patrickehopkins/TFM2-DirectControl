//! Read-only pause-state discovery from the live client UI tree.
//!
//! Teamfight Manager 2 does not expose a dedicated pause bit through the stable client API, but
//! the UI tree is readable. We periodically scan visible label nodes for the game's Resume state
//! and publish only the resulting boolean to the simulation pacer. This keeps the native
//! Candidate-A worker free of UI assumptions.

use std::sync::Mutex;

use mod_api_stable::StableClient;

const SCAN_INTERVAL_MS: u64 = 100;
const MAX_UI_NODES: usize = 2_000;

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
}

impl PauseUiState {
    const fn empty() -> Self {
        Self {
            last_scan_ms: 0,
            paused: false,
            scanned_nodes: 0,
            marker: None,
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

pub fn update(ctx: &StableClient<'_>, ingame: bool) -> PauseUiSnapshot {
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

    if !ingame {
        state.last_scan_ms = 0;
        state.paused = false;
        state.scanned_nodes = 0;
        state.marker = None;
        return state.snapshot();
    }

    if state.last_scan_ms != 0 && now_ms.saturating_sub(state.last_scan_ms) < SCAN_INTERVAL_MS {
        return state.snapshot();
    }
    state.last_scan_ms = now_ms;

    let (paused, scanned_nodes, marker) = scan_visible_pause_text(ctx);
    state.paused = paused;
    state.scanned_nodes = scanned_nodes;
    state.marker = marker;
    state.snapshot()
}

fn scan_visible_pause_text(ctx: &StableClient<'_>) -> (bool, usize, Option<String>) {
    let mut stack = vec![String::new()];
    let mut scanned = 0usize;
    let mut pause_marker: Option<String> = None;

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

            // A hidden parent hides its subtree for our purposes. `None` means the API cannot
            // report visibility for this node, so keep traversing rather than failing closed.
            if matches!(ctx.ui_visible(&path), Some(false)) {
                continue;
            }

            if let Some(text) = ctx.ui_text(&path) {
                let normalized = text.trim().to_ascii_lowercase();
                let is_resume = matches!(normalized.as_str(), "resume" | "resume game" | "unpause");
                let is_pauseish = is_resume || normalized == "pause" || normalized == "paused";

                if is_pauseish && pause_marker.is_none() {
                    pause_marker = Some(format!("{} @ {}", text.trim(), path));
                }
                if is_resume {
                    return (true, scanned, Some(format!("{} @ {}", text.trim(), path)));
                }
            }

            stack.push(path);
        }
    }

    (false, scanned, pause_marker)
}
