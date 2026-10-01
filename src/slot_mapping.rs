//! Maps F1-F10 card positions to authoritative Candidate-A athlete ids.
//!
//! Native follow actions and cards follow the two five-role blocks (Top, Jungle, Mid, Bottom,
//! Support); simulation player-id order is NOT card order. Candidate A supplies athlete/team/lane
//! identities. Stable manager-team identity anchors the F1-F5 own-team block; Candidate A supplies
//! the opposite team for F6-F10. Both teams remain selectable: management identity is an addressing
//! input, not a control restriction. The legacy UI-card matcher is only a fallback if identity is
//! unavailable; duplicate names, remapped follow labels, and hidden cards do not affect the main
//! simulation-based resolution. Fail closed rather than guessing on incomplete data.

use std::sync::Mutex;

use mod_api_stable::{LaneV1, StableClient};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedAthlete {
    athlete_id: usize,
    team: usize,
    lane_index: usize,
}

static AUTHORITATIVE_ROSTER: Mutex<Vec<ObservedAthlete>> = Mutex::new(Vec::new());
// Index zero is F1-F5; index one is F6-F10. Never infer these simulation team
// numbers from the order of Candidate-A player IDs.
static TEAM_BLOCKS: Mutex<[Option<usize>; 2]> = Mutex::new([None, None]);

fn lane_index(lane: LaneV1) -> usize {
    match lane {
        LaneV1::Top => 0,
        LaneV1::Jungle => 1,
        LaneV1::Mid => 2,
        LaneV1::Bottom => 3,
        LaneV1::Support => 4,
    }
}

/// Called from the confirmed Candidate-A AI callback; these are simulation identities, not UI
/// strings. The roster is reset at the Candidate-A job boundary, never per render frame.
pub fn observe_candidate(athlete_id: usize, team: usize, lane: Option<LaneV1>) {
    let Some(lane) = lane else { return };
    let observation = ObservedAthlete {
        athlete_id,
        team,
        lane_index: lane_index(lane),
    };
    if let Ok(mut roster) = AUTHORITATIVE_ROSTER.lock() {
        if !roster.contains(&observation) {
            roster.push(observation);
        }
    }
}

pub fn reset_candidate_roster() {
    if let Ok(mut roster) = AUTHORITATIVE_ROSTER.lock() {
        roster.clear();
    }
    if let Ok(mut blocks) = TEAM_BLOCKS.lock() {
        *blocks = [None, None];
    }
}

pub fn reset() {
    if let Ok(mut state) = LAST.lock() {
        *state = SlotMappingSnapshot::default();
    }
    if let Ok(mut cache) = CACHE.lock() {
        cache.slots.clear();
    }
    // Deliberately retain the Candidate-A observations: this client-scene reset can run after
    // simulation started populating the next match's authoritative roster.
    if let Ok(mut blocks) = TEAM_BLOCKS.lock() {
        *blocks = [None, None];
    }
}

pub fn snapshot() -> SlotMappingSnapshot {
    LAST.lock()
        .map(|state| state.clone())
        .unwrap_or_else(|_| SlotMappingSnapshot {
            error: Some("slot-mapping mutex poisoned".to_owned()),
            ..Default::default()
        })
}

pub fn resolve_fkey(ctx: &StableClient<'_>, fkey_slot: usize) -> Option<usize> {
    if fkey_slot >= SLOT_COUNT {
        publish_error(
            fkey_slot,
            format!("F-key slot {} is outside F1-F10", fkey_slot + 1),
        );
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
            errors
                .get(fkey_slot)
                .and_then(|error| error.clone())
                .unwrap_or_else(|| {
                    format!(
                        "F{} could not be resolved from the visible player cards",
                        fkey_slot + 1
                    )
                }),
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

    // The simulation mapping is stronger than current card text and works when Hide UI removes
    // the label or a remapped native follow shortcut changes its displayed key.
    let roster = authoritative_roster_snapshot();
    if let Some(authoritative_id) = authoritative_slot(fkey_slot, &roster) {
        return (cached.athlete_id == authoritative_id).then_some(cached);
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

fn authoritative_roster_snapshot() -> Vec<ObservedAthlete> {
    AUTHORITATIVE_ROSTER
        .lock()
        .map(|roster| roster.clone())
        .unwrap_or_default()
}

fn complete_authoritative_roster(roster: &[ObservedAthlete]) -> bool {
    if roster.len() != SLOT_COUNT {
        return false;
    }
    let mut ids = std::collections::HashSet::new();
    let mut teams = std::collections::HashMap::<usize, [bool; 5]>::new();
    for entry in roster {
        if !ids.insert(entry.athlete_id) {
            return false;
        }
        let lanes = teams.entry(entry.team).or_insert([false; 5]);
        if lanes[entry.lane_index] {
            return false;
        }
        lanes[entry.lane_index] = true;
    }
    teams.len() == 2 && teams.values().all(|lanes| lanes.iter().all(|seen| *seen))
}

fn calibrated_team_blocks(
    ui_slots: &[Option<CachedSlot>],
    roster: &[ObservedAthlete],
    manager_team_id: Option<usize>,
) -> [Option<usize>; 2] {
    // Own-team F1-F5, enemy F6-F10: identify both from management identity and Candidate A.
    // Never guess from player-id order, name strings, or labels such as "(F1)".
    if complete_authoritative_roster(roster) {
        if let Some(own) = manager_team_id {
            if roster.iter().any(|entry| entry.team == own) {
                if let Some(enemy) = roster
                    .iter()
                    .map(|entry| entry.team)
                    .find(|team| *team != own)
                {
                    let blocks = [Some(own), Some(enemy)];
                    if let Ok(mut saved) = TEAM_BLOCKS.lock() {
                        *saved = blocks;
                    }
                    return blocks;
                }
            }
        }
    }

    let previous = TEAM_BLOCKS
        .lock()
        .map(|blocks| *blocks)
        .unwrap_or([None, None]);
    let mut blocks = previous;
    let mut conflicts = [false, false];

    // Each trustworthy UI card votes for the team occupying its block only when its matched
    // athlete also has the card's expected role in the authoritative roster.
    for (fkey_slot, slot) in ui_slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        let role = fkey_slot % 5;
        let matches = roster
            .iter()
            .filter(|entry| entry.athlete_id == slot.athlete_id && entry.lane_index == role)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            continue;
        }
        let block = fkey_slot / 5;
        let team = matches[0].team;
        if let Some(existing) = blocks[block] {
            if existing != team {
                conflicts[block] = true;
            }
        } else {
            blocks[block] = Some(team);
        }
    }
    for block in 0..2 {
        if conflicts[block] {
            blocks[block] = None;
        }
    }
    if conflicts.iter().any(|conflict| *conflict) {
        return [None, None];
    }

    // Exactly two complete teams, and at least one positively identified block, lets us infer
    // the opposite block without consulting its potentially duplicate/hidden card names.
    if complete_authoritative_roster(roster) {
        let team_a = roster[0].team;
        let team_b = roster
            .iter()
            .find(|entry| entry.team != team_a)
            .map(|entry| entry.team)
            .unwrap_or(team_a);
        match blocks {
            [Some(team), None] => {
                blocks[1] = Some(if team == team_a { team_b } else { team_a });
            }
            [None, Some(team)] => {
                blocks[0] = Some(if team == team_a { team_b } else { team_a });
            }
            _ => {}
        }
    }

    if blocks[0].is_some() && blocks[0] == blocks[1] {
        return [None, None];
    }
    if let Ok(mut saved) = TEAM_BLOCKS.lock() {
        *saved = blocks;
    }
    blocks
}

fn authoritative_slot_with_blocks(
    fkey_slot: usize,
    roster: &[ObservedAthlete],
    blocks: [Option<usize>; 2],
) -> Option<usize> {
    if fkey_slot >= SLOT_COUNT || !complete_authoritative_roster(roster) {
        return None;
    }
    let team = blocks[fkey_slot / 5]?;
    roster
        .iter()
        .find(|entry| entry.team == team && entry.lane_index == fkey_slot % 5)
        .map(|entry| entry.athlete_id)
}

fn authoritative_slot(fkey_slot: usize, roster: &[ObservedAthlete]) -> Option<usize> {
    let blocks = TEAM_BLOCKS.lock().map(|blocks| *blocks).ok()?;
    authoritative_slot_with_blocks(fkey_slot, roster, blocks)
}

fn populate_authoritative_slots(
    ctx: &StableClient<'_>,
    slots: &mut [Option<CachedSlot>],
    errors: &mut [Option<String>],
    roster: &[ObservedAthlete],
    blocks: [Option<usize>; 2],
) {
    for fkey_slot in 0..SLOT_COUNT {
        let Some(athlete_id) = authoritative_slot_with_blocks(fkey_slot, roster, blocks) else {
            continue;
        };
        let athlete_name = ctx.athlete_name(athlete_id).unwrap_or_default();
        slots[fkey_slot] = Some(CachedSlot {
            card_text: format!(
                "authoritative simulation identity: team={}, role={}",
                blocks[fkey_slot / 5].unwrap_or_default(),
                fkey_slot % 5
            ),
            card_path: "<simulation roster>".to_owned(),
            athlete_name,
            athlete_id,
        });
        errors[fkey_slot] = None;
    }
}

fn rebuild_mapping(ctx: &StableClient<'_>) -> (Vec<Option<CachedSlot>>, Vec<Option<String>>) {
    let card_candidates = scan_visible_card_text(ctx);
    let athletes = athlete_names(ctx);
    let mut slots = vec![None; SLOT_COUNT];
    let mut errors = vec![None; SLOT_COUNT];

    if athletes.is_empty() {
        for (slot, error) in errors.iter_mut().enumerate() {
            *error = Some(format!(
                "F{}: stable API supplied no named athletes",
                slot + 1
            ));
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
                fkey_slot + 1,
                fkey_slot + 1
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
                fkey_slot + 1,
                candidates.len()
            ));
            continue;
        }

        // Mirrored UI nodes are okay only if they all agree on athlete identity.
        let first_athlete = resolved[0].athlete_id;
        if resolved
            .iter()
            .any(|candidate| candidate.athlete_id != first_athlete)
        {
            errors[fkey_slot] = Some(format!(
                "F{}: multiple different athletes matched its visible card nodes",
                fkey_slot + 1
            ));
            continue;
        }
        resolved.sort_by_key(|candidate| {
            (
                candidate.card_path.matches('.').count(),
                candidate.card_path.len(),
                candidate.card_text.len(),
            )
        });
        slots[fkey_slot] = Some(resolved.remove(0));
    }

    // First reject ambiguous presentation matches. They must never serve as calibration anchors.
    reject_duplicate_assignments(&mut slots, &mut errors);
    let roster = authoritative_roster_snapshot();
    let blocks = calibrated_team_blocks(&slots, &roster, ctx.player_team_id());
    populate_authoritative_slots(ctx, &mut slots, &mut errors, &roster, blocks);
    reject_duplicate_assignments(&mut slots, &mut errors);

    (slots, errors)
}

// Do not keep whichever conflicting card happened to be scanned first.
fn reject_duplicate_assignments(slots: &mut [Option<CachedSlot>], errors: &mut [Option<String>]) {
    let mut first_slot_for_athlete = std::collections::HashMap::new();
    for fkey_slot in 0..slots.len() {
        let Some(id) = slots[fkey_slot].as_ref().map(|slot| slot.athlete_id) else {
            continue;
        };
        if let Some(&previous) = first_slot_for_athlete.get(&id) {
            errors[fkey_slot] = Some(format!(
                "F{}: athlete also appears on F{}; refusing ambiguous assignment",
                fkey_slot + 1,
                previous + 1
            ));
            errors[previous] = Some(format!(
                "F{}: athlete also appears on F{}; refusing ambiguous assignment",
                previous + 1,
                fkey_slot + 1
            ));
            slots[fkey_slot] = None;
            slots[previous] = None;
        } else {
            first_slot_for_athlete.insert(id, fkey_slot);
        }
    }
}

/// Read-only automatic one-time roster health check for support logs.
pub fn roster_diagnostics(ctx: &StableClient<'_>) -> String {
    let (slots, errors) = rebuild_mapping(ctx);
    let resolved = slots.iter().filter(|slot| slot.is_some()).count();
    let failures = errors
        .iter()
        .filter_map(|error| error.as_deref())
        .collect::<Vec<_>>();
    let roster = authoritative_roster_snapshot();
    let blocks = TEAM_BLOCKS
        .lock()
        .map(|blocks| *blocks)
        .unwrap_or([None, None]);
    let authority = format!(
        "candidate_a_roster={}/10 complete={} team_blocks={blocks:?} manager_team_id={:?}",
        roster.len(),
        complete_authoritative_roster(&roster),
        ctx.player_team_id()
    );
    if failures.is_empty() {
        return format!(
            "roster probe: {resolved}/10 slots identified; {authority}; all selection mappings available"
        );
    }
    format!(
        "roster probe: {resolved}/10 slots identified; {authority}; {}",
        failures.join("; ")
    )
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

            // Hidden is not absent: the player may disable the HUD. Read accessible nodes even
            // when they are not being rendered; never mutate the game's UI to expose them.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn card(athlete_id: usize) -> Option<CachedSlot> {
        Some(CachedSlot {
            card_text: format!("Player (F{})", athlete_id + 1),
            card_path: format!("ingame.player_card.{}", athlete_id),
            athlete_name: format!("Player {}", athlete_id),
            athlete_id,
        })
    }

    #[test]
    fn unrelated_missing_card_does_not_block_valid_slots() {
        let mut slots = vec![card(1), None, card(2)];
        let mut errors = vec![None; slots.len()];
        reject_duplicate_assignments(&mut slots, &mut errors);
        assert!(slots[0].is_some());
        assert!(slots[1].is_none());
        assert!(slots[2].is_some());
        assert!(errors.iter().all(Option::is_none));
    }

    #[test]
    fn duplicate_athlete_disables_all_conflicting_keys_only() {
        let mut slots = vec![card(1), card(2), card(1), card(3), card(1)];
        let mut errors = vec![None; slots.len()];
        reject_duplicate_assignments(&mut slots, &mut errors);
        assert!(slots[0].is_none() && slots[2].is_none() && slots[4].is_none());
        assert!(slots[1].is_some() && slots[3].is_some());
        assert!(errors[0].is_some() && errors[2].is_some() && errors[4].is_some());
    }

    #[test]
    fn duplicate_names_do_not_affect_authoritative_mapping() {
        let roster = vec![
            ObservedAthlete {
                athlete_id: 10,
                team: 0,
                lane_index: 0,
            },
            ObservedAthlete {
                athlete_id: 11,
                team: 0,
                lane_index: 1,
            },
            ObservedAthlete {
                athlete_id: 12,
                team: 0,
                lane_index: 2,
            },
            ObservedAthlete {
                athlete_id: 13,
                team: 0,
                lane_index: 3,
            },
            ObservedAthlete {
                athlete_id: 14,
                team: 0,
                lane_index: 4,
            },
            ObservedAthlete {
                athlete_id: 20,
                team: 1,
                lane_index: 0,
            },
            ObservedAthlete {
                athlete_id: 21,
                team: 1,
                lane_index: 1,
            },
            ObservedAthlete {
                athlete_id: 22,
                team: 1,
                lane_index: 2,
            },
            ObservedAthlete {
                athlete_id: 23,
                team: 1,
                lane_index: 3,
            },
            ObservedAthlete {
                athlete_id: 24,
                team: 1,
                lane_index: 4,
            },
        ];
        assert!(complete_authoritative_roster(&roster));
        let blocks = [Some(0), Some(1)];
        assert_eq!(authoritative_slot_with_blocks(0, &roster, blocks), Some(10));
        assert_eq!(authoritative_slot_with_blocks(5, &roster, blocks), Some(20));
        assert_eq!(
            authoritative_slot_with_blocks(0, &roster, [None, None]),
            None
        );
    }

    #[test]
    fn all_labels_remapped_and_names_duplicated_still_resolve_both_teams() {
        reset_candidate_roster();
        let roster = (0..10)
            .map(|i| ObservedAthlete {
                athlete_id: 100 + i,
                team: if i < 5 { 9 } else { 3 },
                lane_index: i % 5,
            })
            .collect::<Vec<_>>();
        let no_card_labels = vec![None; SLOT_COUNT];
        let blocks = calibrated_team_blocks(&no_card_labels, &roster, Some(9));
        assert_eq!(blocks, [Some(9), Some(3)]);
        for i in 0..10 {
            assert_eq!(
                authoritative_slot_with_blocks(i, &roster, blocks),
                Some(100 + i)
            );
        }
        // The manager's team need not have the lowest numeric ID.
        let reversed = calibrated_team_blocks(&no_card_labels, &roster, Some(3));
        assert_eq!(reversed, [Some(3), Some(9)]);
        assert_eq!(
            authoritative_slot_with_blocks(0, &roster, reversed),
            Some(105)
        );
        assert_eq!(
            authoritative_slot_with_blocks(5, &roster, reversed),
            Some(100)
        );
        reset_candidate_roster();
    }

    #[test]
    fn unavailable_team_identity_does_not_guess_an_initial_block() {
        reset_candidate_roster();
        let roster = (0..10)
            .map(|i| ObservedAthlete {
                athlete_id: 100 + i,
                team: if i < 5 { 9 } else { 3 },
                lane_index: i % 5,
            })
            .collect::<Vec<_>>();
        let no_card_labels = vec![None; SLOT_COUNT];
        assert_eq!(
            calibrated_team_blocks(&no_card_labels, &roster, None),
            [None, None]
        );
        assert_eq!(
            calibrated_team_blocks(&no_card_labels, &roster, Some(777)),
            [None, None]
        );
        reset_candidate_roster();
    }

    #[test]
    fn longest_unambiguous_name_wins_in_card_text() {
        let athletes = vec![(1, "Sam".to_owned()), (2, "Samwise".to_owned())];
        let selection = match_card_to_athlete("Samwise (F2)", &athletes).unwrap();
        assert_eq!(selection.0, 2);
    }
}
