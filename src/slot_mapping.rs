//! Team-neutral translation from the visible F1-F10 player cards to stable athlete ids.
//!
//! The match UI orders ten cards and labels them with `(F1)` ... `(F10)`, but runtime testing
//! proved that this visible order is not the same as Candidate A's internal `player_id` order.
//! StableClient can resolve management athlete ids to names, so we match the visible card text to
//! the athlete name and then control by `StableAiContext::athlete_id()` on the simulation thread.

use std::sync::Mutex;

use mod_api_stable::StableClient;

const MAX_UI_NODES: usize = 2_000;

#[derive(Debug, Clone, Default)]
pub struct SlotMappingSnapshot {
    pub fkey_slot: Option<usize>,
    pub card_text: Option<String>,
    pub card_path: Option<String>,
    pub athlete_name: Option<String>,
    pub athlete_id: Option<usize>,
    pub error: Option<String>,
}

static LAST: Mutex<SlotMappingSnapshot> = Mutex::new(SlotMappingSnapshot {
    fkey_slot: None,
    card_text: None,
    card_path: None,
    athlete_name: None,
    athlete_id: None,
    error: None,
});

pub fn reset() {
    if let Ok(mut state) = LAST.lock() {
        *state = SlotMappingSnapshot::default();
    }
}

pub fn snapshot() -> SlotMappingSnapshot {
    LAST.lock().map(|state| state.clone()).unwrap_or_else(|_| SlotMappingSnapshot {
        error: Some("slot-mapping mutex poisoned".to_owned()),
        ..Default::default()
    })
}

pub fn resolve_fkey(ctx: &StableClient<'_>, fkey_slot: usize) -> Option<usize> {
    let key_suffix = format!("(F{})", fkey_slot + 1);
    let (card_text, card_path) = match find_card_text(ctx, &key_suffix) {
        Some(found) => found,
        None => {
            publish(SlotMappingSnapshot {
                fkey_slot: Some(fkey_slot),
                error: Some(format!("no visible player-card text containing {key_suffix}")),
                ..Default::default()
            });
            return None;
        }
    };

    let card_lower = card_text.to_ascii_lowercase();
    let mut best: Option<(usize, String)> = None;

    for athlete_id in ctx.athlete_ids() {
        let Some(name) = ctx.athlete_name(athlete_id) else {
            continue;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }

        let name_lower = trimmed.to_ascii_lowercase();
        if card_lower.contains(&name_lower)
            && best
                .as_ref()
                .map(|(_, previous)| trimmed.len() > previous.len())
                .unwrap_or(true)
        {
            best = Some((athlete_id, trimmed.to_owned()));
        }
    }

    let Some((athlete_id, athlete_name)) = best else {
        publish(SlotMappingSnapshot {
            fkey_slot: Some(fkey_slot),
            card_text: Some(card_text),
            card_path: Some(card_path),
            error: Some("card found, but no StableClient athlete name matched it".to_owned()),
            ..Default::default()
        });
        return None;
    };

    publish(SlotMappingSnapshot {
        fkey_slot: Some(fkey_slot),
        card_text: Some(card_text),
        card_path: Some(card_path),
        athlete_name: Some(athlete_name),
        athlete_id: Some(athlete_id),
        error: None,
    });
    Some(athlete_id)
}

fn publish(snapshot: SlotMappingSnapshot) {
    if let Ok(mut state) = LAST.lock() {
        *state = snapshot;
    }
}

fn find_card_text(ctx: &StableClient<'_>, key_suffix: &str) -> Option<(String, String)> {
    let mut stack = vec![String::new()];
    let mut scanned = 0usize;

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
                let trimmed = text.trim();
                if trimmed.contains(key_suffix) {
                    return Some((trimmed.to_owned(), path));
                }
            }

            stack.push(path);
        }
    }

    None
}
