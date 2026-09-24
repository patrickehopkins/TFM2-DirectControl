//! Temporary release diagnostic: inspect the actual match UI tree rather than hiding buttons
//! solely because their rectangles happen to fall under the top-left HUD.
//!
//! Hold Ctrl+Shift+F12 while TFM2 is the foreground window during an InGame match.
//! A bounded report is written to %TEMP%\\tfm2_replay_ui_probe.txt.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

use mod_api_stable::{ClientSceneKindV1, StableClient};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_F12, VK_SHIFT,
};

use crate::input_focus;

static CAPTURE_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);

fn interesting(path: &str, runner: &str, rect: Option<(f32, f32, f32, f32)>) -> bool {
    let lower = path.to_ascii_lowercase();
    let name_match = [
        "zoom", "replay", "seek", "highlight", "speed", "camera",
        "timeline", "tooltip", "playback", "forward", "back", "header",
    ]
    .iter()
    .any(|word| lower.contains(word));
    let top_control = rect.is_some_and(|(x, y, w, h)| {
        (0.0..=550.0).contains(&(x + w * 0.5))
            && (25.0..=185.0).contains(&(y + h * 0.5))
            && (runner.contains("button") || runner.contains("selectable"))
    });
    name_match || top_control
}

pub fn maybe_capture(ctx: &StableClient<'_>, hidden_nodes: &[(String, bool)]) {
    if !matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame)) {
        CAPTURE_CHORD_WAS_DOWN.store(false, Ordering::Release);
        return;
    }

    if !input_focus::process_owns_foreground_window() {
        CAPTURE_CHORD_WAS_DOWN.store(true, Ordering::Release);
        return;
    }

    let chord_down = unsafe {
        GetAsyncKeyState(VK_CONTROL as i32) < 0
            && GetAsyncKeyState(VK_SHIFT as i32) < 0
            && GetAsyncKeyState(VK_F12 as i32) < 0
    };
    let was_down = CAPTURE_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
    if !chord_down || was_down {
        return;
    }

    let mut report = String::from(
        "TFM2 Replay UI Probe — Ctrl+Shift+F12\\n\\n         The listed rectangles are logical game UI coordinates. UI paths are read-only.\\n         The probe does not modify the game or disable shortcuts.\\n\\n",
    );
    let _ = writeln!(
        report,
        "Current Direct Control seek-suppression candidates ({}):",
        hidden_nodes.len()
    );
    for (path, original_visibility) in hidden_nodes {
        let _ = writeln!(
            report,
            "  {path} | originally_visible={original_visibility} | now_visible={:?} | runner={:?} | rect={:?}",
            ctx.ui_visible(path), ctx.ui_runner_name(path), ctx.ui_node_rect(path)
        );
    }
    report.push_str("\\nRelevant live UI nodes:\\n");

    let mut visited = HashSet::new();
    let mut pending = vec![("".to_owned(), 0usize), ("ingame".to_owned(), 0usize)];
    let mut count = 0usize;
    const MAX_NODES: usize = 8_000;
    const MAX_DEPTH: usize = 10;

    while let Some((parent, depth)) = pending.pop() {
        if depth >= MAX_DEPTH || count >= MAX_NODES {
            continue;
        }
        for child in ctx.ui_child_names(&parent) {
            if count >= MAX_NODES {
                break;
            }
            let path = if parent.is_empty() {
                child
            } else {
                format!("{parent}.{child}")
            };
            if !visited.insert(path.clone()) {
                continue;
            }
            count += 1;

            let runner = ctx.ui_runner_name(&path).unwrap_or_default();
            let rect = ctx.ui_node_rect(&path);
            if interesting(&path, &runner, rect) {
                let visible = ctx.ui_visible(&path);
                let label = ctx.ui_text(&path).unwrap_or_default();
                let _ = writeln!(
                    report,
                    "{path} | runner={runner:?} | visible={visible:?} | rect={rect:?} | text={label:?}"
                );
            }
            if depth + 1 < MAX_DEPTH {
                pending.push((path, depth + 1));
            }
        }
    }

    let _ = writeln!(report, "\\nTraversed {count} unique UI nodes.");
    let output = std::env::temp_dir().join("tfm2_replay_ui_probe.txt");
    let _ = fs::write(output, report);
}
