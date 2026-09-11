//! Read-only pause-state discovery from the live client UI.
//!
//! There are two distinct ways the match presentation can stop while the InGame scene remains
//! active:
//!
//! 1. the ordinary full-screen `pause_ui`; and
//! 2. the match viewer's timeline/playback pause, where the speed selector has no active speed.
//!
//! Candidate A must stop for either one. Otherwise the simulation continues at 60 Hz while the
//! viewer is stationary and direct-control input once again targets state ahead of what the player
//! can see.
//!
//! The speed buttons are exposed as selectable widgets by the stable UI API. We only infer a
//! timeline pause when at least one speed widget is recognized and *none* of the recognized speed
//! widgets is selected. If a future game version changes those widget types/paths, this probe fails
//! open instead of inventing a pause.

use mod_api_stable::StableClient;

const MAX_UI_NODES: usize = 2_000;
const SPEED_BUTTON_PATHS: [&str; 5] = [
    "speed_buttons.speed05x",
    "speed_buttons.speed1x",
    "speed_buttons.speed15x",
    "speed_buttons.speed2x",
    "speed_buttons.speed3x",
];

#[derive(Debug, Clone)]
pub struct PauseUiSnapshot {
    pub paused: bool,
    pub scanned_nodes: usize,
    pub marker: Option<String>,
}

pub fn reset() {}

pub fn update(ctx: &StableClient<'_>, interactive_match: bool) -> PauseUiSnapshot {
    if !interactive_match {
        return PauseUiSnapshot {
            paused: false,
            scanned_nodes: 0,
            marker: None,
        };
    }

    if matches!(ctx.ui_visible("pause_ui"), Some(true)) {
        let marker = match ctx.ui_node_rect("pause_ui") {
            Some((x, y, w, h)) => Some(format!(
                "pause_ui [direct visible {:.0}x{:.0} @ {:.0},{:.0}]",
                w, h, x, y
            )),
            None => Some("pause_ui [direct visible]".to_owned()),
        };
        return PauseUiSnapshot {
            paused: true,
            scanned_nodes: 1,
            marker,
        };
    }

    if let Some(snapshot) = timeline_pause_state(ctx) {
        if snapshot.paused {
            return snapshot;
        }
    }

    let (paused, scanned_nodes, marker) = scan_visible_resume_text(ctx);
    PauseUiSnapshot {
        paused,
        scanned_nodes,
        marker,
    }
}

fn timeline_pause_state(ctx: &StableClient<'_>) -> Option<PauseUiSnapshot> {
    let mut recognized = 0usize;
    let mut selected = 0usize;
    let mut selected_name: Option<&str> = None;

    for path in SPEED_BUTTON_PATHS {
        let Some(is_selected) = ctx.ui_selectable_selected(path) else {
            continue;
        };

        recognized += 1;
        if is_selected {
            selected += 1;
            selected_name = Some(path);
        }
    }

    if recognized == 0 {
        return None;
    }

    if selected == 0 {
        return Some(PauseUiSnapshot {
            paused: true,
            scanned_nodes: recognized,
            marker: Some(format!(
                "timeline paused [0/{recognized} recognized speed buttons selected]"
            )),
        });
    }

    Some(PauseUiSnapshot {
        paused: false,
        scanned_nodes: recognized,
        marker: Some(format!(
            "timeline running [{} selected]",
            selected_name.unwrap_or("speed")
        )),
    })
}

fn scan_visible_resume_text(ctx: &StableClient<'_>) -> (bool, usize, Option<String>) {
    let mut stack = vec![String::new()];
    let mut scanned = 0usize;
    let mut best_hint: Option<String> = None;

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

            if let Some(text) = ctx.ui_text(&path) {
                let normalized = text.trim().to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "return to game" | "resume" | "resume game" | "unpause"
                ) {
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
                    return (true, scanned, Some(format!("{} [state]", path)));
                }
            }

            stack.push(path);
        }
    }

    (false, scanned, best_hint)
}
