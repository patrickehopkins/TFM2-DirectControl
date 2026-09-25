//! Team-neutral translation from the visible F1-F10 player cards to stable athlete ids.
//!
//! Runtime testing proved that visible card order is not Candidate A's internal `player_id` order.
//! Keep F1-F10 defined by the visible cards, but resolve the whole ten-card roster coherently:
//!
//! visible F-key card -> displayed athlete identity -> stable athlete id
//!
//! The mapping is cached per match and revalidated against the live UI before reuse. A rebuild scans
//! the UI once but resolves each slot independently: an unrelated broken card must never prevent
//! selection of a safely resolved slot. Ambiguous or duplicated athlete assignments are rejected.
//! This is intentionally preferred over the
//! native Follow Own/Enemy action registry: those actions are role-oriented (top/jungle/mid/etc.)
//! and do not directly expose the stable athlete id required by the control layer.

use std::{
    sync::Mutex,
};

use mod_api_stable::StableClient;

const MAX_UI_NODES: usize = 2_000;
const SLOT_COUNT: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct SlotMappingSnapshot {
    pub fkey_slot: Option<usize>,
    pub card_text: Option<String>,
    pub card_path: Option<String>,
    pub athlete_name: Option<String>,
    pub athlete_id: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedSlot {
    card_text: String,
    card_path: String,
    athlete_name: String,
    athlete_id: usize,
}

#[derive(Debug, Default)]
struct MappingCache {
    slots: Vec<Option<CachedSlot>>,
}

static LAST: Mutex<SlotMappingSnapshot> = Mutex::new(SlotMappingSnapshot {
    fkey_slot: None,
    card_text: None,
    card_path: None,
    athlete_name: None,
    athlete_id: None,
    error: None,
});

static CACHE: Mutex<MappingCache> = Mutex::new(MappingCache { slots: Vec::new() });

pub fn reset() {
    if let Ok(mut state) = LAST.lock() {
        *state = SlotMappingSnapshot::default();
    }
    if let Ok(mut cache) = CACHE.lock() {
        cache.slots.clear();
    }
}

pub fn snapshot() -> SlotMappingSnapshot {
    LAST.lock().map(|state| state.clone()).unwrap_or_else(|_| SlotMappingSnapshot {
        error: Some("slot-mapping mutex poisoned".to_owned()),
        ..Default::default()
    })
}

pub fn resolve_fkey(ctx: &StableClient<'_>, fkey_slot: usize) -> Option<usize> {
    if fkey_slot >= SLOT_COUNT {
        publish_error(fkey_slot, format!("F-key slot {} is outside F1-F10", fkey_slot + 1));
        return None;
    }

    if let Some(cached) = cached_slot_if_valid(ctx, fkey_slot) {
        publish_success(fkey_slot, &cached);
        return Some(cached.athlete_id);
    }

    let (rebuilt, errors) = rebuild_mapping(ctx);
    let selected = rebuilt.get(fkey_slot).and_then(|slot| slot.clone());
    if let Ok(mut cache) = CACHE.lock() {
        cache.slots = rebuilt;
    }

    let Some(selected) = selected else {
        publish_error(
            fkey_slot,
            errors.get(fkey_slot).and_then(|error| error.clone()).unwrap_or_else(||
                format!("F{} could not be resolved from the visible player cards", fkey_slot + 1)),
        );
        return None;
    };

    publish_success(fkey_slot, &selected);
    Some(selected.athlete_id)
}

fn publish_success(fkey_slot: usize, slot: &CachedSlot) {
    publish(SlotMappingSnapshot {
        fkey_slot: Some(fkey_slot),
        card_text: Some(slot.card_text.clone()),
        card_path: Some(slot.card_path.clone()),
        athlete_name: Some(slot.athlete_name.clone()),
        athlete_id: Some(slot.athlete_id),
        error: None,
    });
}

fn publish_error(fkey_slot: usize, error: String) {
    publish(SlotMappingSnapshot {
        fkey_slot: Some(fkey_slot),
        error: Some(error),
        ..Default::default()
    });
}

fn publish(snapshot: SlotMappingSnapshot) {
    if let Ok(mut state) = LAST.lock() {
        *state = snapshot;
    }
}

fn cached_slot_if_valid(ctx: &StableClient<'_>, fkey_slot: usize) -> Option<CachedSlot> {
    let cached = CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.slots.get(fkey_slot).cloned().flatten())?;

    if matches!(ctx.ui_visible(&cached.card_path), Some(false)) {
        return None;
    }

    let current = ctx.ui_text(&cached.card_path)?;
    let trimmed = current.trim();
    let key_suffix = format!("(F{})", fkey_slot + 1);
    if !trimmed.contains(&key_suffix)
        || !trimmed
            .to_ascii_lowercase()
            .contains(&cached.athlete_name.to_ascii_lowercase())
    {
        return None;
    }

    Some(CachedSlot {
        card_text: trimmed.to_owned(),
        ..cached
    })
}

fn rebuild_mapping(ctx: &StableClient<'_>) -> (Vec<Option<CachedSlot>>, Vec<Option<String>>) {
    let card_candidates = scan_visible_card_text(ctx);
    let athletes = athlete_names(ctx);
    let mut slots = vec![None; SLOT_COUNT];
    let mut errors = vec![None; SLOT_COUNT];

    if athletes.is_empty() {
        for (slot, error) in errors.iter_mut().enumerate() {
            *error = Some(format!("F{}: stable API supplied no named athletes", slot + 1));
        }
        return (slots, errors);
    }

    // Resolve every slot independently. In particular, an extra UI panel with one
    // missing card should not disable all nine other champions.
    for fkey_slot in 0..SLOT_COUNT {
        let candidates = &card_candidates[fkey_slot];
        if candidates.is_empty() {
            errors[fkey_slot] = Some(format!(
                "F{}: no visible player-card text containing (F{})",
                fkey_slot + 1, fkey_slot + 1
            ));
            continue;
        }

        let mut resolved: Vec<CachedSlot> = Vec::new();
        for (card_text, card_path) in candidates {
            if let Ok((athlete_id, athlete_name)) = match_card_to_athlete(card_text, &athletes) {
                resolved.push(CachedSlot {
                    card_text: card_text.clone(),
                    card_path: card_path.clone(),
                    athlete_name,
                    athlete_id,
                });
            }
        }
        if resolved.is_empty() {
            errors[fkey_slot] = Some(format!(
                "F{}: {} visible card candidate(s), no unambiguous athlete-name match",
                fkey_slot + 1, candidates.len()
            ));
            continue;
        }

        // Mirrored UI nodes are okay only if they all agree on athlete identity.
        let first_athlete = resolved[0].athlete_id;
        if resolved.iter().any(|candidate| candidate.athlete_id != first_athlete) {
            errors[fkey_slot] = Some(format!(
                "F{}: multiple different athletes matched its visible card nodes",
                fkey_slot + 1
            ));
            continue;
        }
        resolved.sort_by_key(|candidate| (
            candidate.card_path.matches('.').count(),
            candidate.card_path.len(),
            candidate.card_text.len(),
        ));
        slots[fkey_slot] = Some(resolved.remove(0));
    }

    // Reject BOTH sides of an ambiguous duplicate; keeping whichever was
    // encountered first would silently let F-keys control the wrong athlete.
    let mut first_slot_for_athlete = std::collections::HashMap::new();
    for fkey_slot in 0..SLOT_COUNT {
        let Some(id) = slots[fkey_slot].as_ref().map(|slot| slot.athlete_id) else { continue; };
        if let Some(&previous) = first_slot_for_athlete.get(&id) {
            errors[fkey_slot] = Some(format!(
                "F{}: athlete also appears on F{}; refusing ambiguous assignment",
                fkey_slot + 1, previous + 1
            ));
            errors[previous] = Some(format!(
                "F{}: athlete also appears on F{}; refusing ambiguous assignment",
                previous + 1, fkey_slot + 1
            ));
            slots[fkey_slot] = None;
            slots[previous] = None;
        } else {
            first_slot_for_athlete.insert(id, fkey_slot);
        }
    }

    (slots, errors)
}

fn athlete_names(ctx: &StableClient<'_>) -> Vec<(usize, String)> {
    let mut athletes = Vec::new();
    for athlete_id in ctx.athlete_ids() {
        let Some(name) = ctx.athlete_name(athlete_id) else {
            continue;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        athletes.push((athlete_id, trimmed.to_owned()));
    }
    athletes
}

fn match_card_to_athlete(
    card_text: &str,
    athletes: &[(usize, String)],
) -> Result<(usize, String), String> {
    let card_lower = card_text.to_ascii_lowercase();
    let mut matches: Vec<(usize, String)> = athletes
        .iter()
        .filter_map(|(athlete_id, name)| {
            card_lower
                .contains(&name.to_ascii_lowercase())
                .then_some((*athlete_id, name.clone()))
        })
        .collect();

    if matches.is_empty() {
        return Err("no athlete name appears in card text".to_owned());
    }

    let longest = matches
        .iter()
        .map(|(_, name)| name.len())
        .max()
        .unwrap_or(0);
    matches.retain(|(_, name)| name.len() == longest);

    if matches.len() != 1 {
        return Err(format!(
            "athlete-name match is ambiguous at longest length {longest}"
        ));
    }

    Ok(matches.remove(0))
}

fn scan_visible_card_text(ctx: &StableClient<'_>) -> Vec<Vec<(String, String)>> {
    let mut slots = vec![Vec::new(); SLOT_COUNT];
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
                if !trimmed.is_empty() {
                    for fkey_slot in 0..SLOT_COUNT {
                        let key_suffix = format!("(F{})", fkey_slot + 1);
                        if trimmed.contains(&key_suffix) {
                            slots[fkey_slot].push((trimmed.to_owned(), path.clone()));
                        }
                    }
                }
            }

            stack.push(path);
        }
    }

    slots
}
