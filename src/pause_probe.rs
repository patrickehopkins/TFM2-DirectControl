//! Read-only pause-state discovery from the live client UI.
//!
//! Runtime testing identified `pause_ui` as the actual full-screen pause root. Stage 4C's
//! clock-stall fallback was intentionally removed because it could deadlock resume: once Candidate
//! A was blocked, the visible clock could not advance to prove that the pause had ended.
//!
//! The direct pause root is therefore the primary signal. A small text/state traversal remains only
//! as a compatibility fallback if a later UI build stops exposing that exact root path.

use mod_api_stable::StableClient;

const MAX_UI_NODES: usize = 2_000;

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

    let (paused, scanned_nodes, marker) = scan_visible_resume_text(ctx);
    PauseUiSnapshot {
        paused,
        scanned_nodes,
        marker,
    }
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
