//! Minimal cross-thread manual-control state for the paced Candidate-A simulation.
//!
//! Physical F-key selection is resolved by the client to an athlete identity. The Candidate-A AI
//! hook then compares `StableAiContext::athlete_id()` rather than assuming the visible F-key order
//! matches the simulation's internal `player_id` order. This remains team-neutral and avoids
//! hard-coded side/order policy.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use mod_api_stable::InputV1;

const NO_ATHLETE: usize = usize::MAX;
const NO_TICK: u64 = u64::MAX;

static SELECTED_ATHLETE: AtomicUsize = AtomicUsize::new(NO_ATHLETE);
static MOVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static MOVE_X: AtomicU64 = AtomicU64::new(0);
static MOVE_Y: AtomicU64 = AtomicU64::new(0);
static MOVE_VERSION: AtomicU64 = AtomicU64::new(0);

static SELECT_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_COMMAND_COUNT: AtomicU64 = AtomicU64::new(0);
static MANUAL_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static LAST_MANUAL_TICK: AtomicU64 = AtomicU64::new(NO_TICK);

#[derive(Debug, Clone, Copy)]
pub struct ControlDiagnostics {
    pub selected_athlete: Option<usize>,
    pub move_target: Option<(u64, u64)>,
    pub select_count: u64,
    pub move_command_count: u64,
    pub manual_input_returns: u64,
    pub last_manual_tick: Option<u64>,
}

pub fn reset() {
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    SELECT_COUNT.store(0, Ordering::Release);
    MOVE_COMMAND_COUNT.store(0, Ordering::Release);
    MANUAL_INPUT_RETURNS.store(0, Ordering::Release);
    LAST_MANUAL_TICK.store(NO_TICK, Ordering::Release);
}

pub fn selected_athlete() -> Option<usize> {
    match SELECTED_ATHLETE.load(Ordering::Acquire) {
        NO_ATHLETE => None,
        athlete_id => Some(athlete_id),
    }
}

pub fn select_athlete(athlete_id: usize) {
    // Clear the old destination before publishing the new identity so the newly selected athlete
    // can never inherit the previous athlete's persistent MoveTo target.
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    SELECTED_ATHLETE.store(athlete_id, Ordering::Release);
    SELECT_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn publish_move_target(x: u64, y: u64) {
    // Tiny seqlock for a coherent coordinate pair across render and simulation threads.
    MOVE_VERSION.fetch_add(1, Ordering::AcqRel); // odd = write in progress
    MOVE_X.store(x, Ordering::Relaxed);
    MOVE_Y.store(y, Ordering::Relaxed);
    MOVE_ACTIVE.store(true, Ordering::Relaxed);
    MOVE_VERSION.fetch_add(1, Ordering::Release); // even = stable snapshot
    MOVE_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn clear_move_target() {
    MOVE_VERSION.fetch_add(1, Ordering::AcqRel);
    MOVE_ACTIVE.store(false, Ordering::Relaxed);
    MOVE_X.store(0, Ordering::Relaxed);
    MOVE_Y.store(0, Ordering::Relaxed);
    MOVE_VERSION.fetch_add(1, Ordering::Release);
}

pub fn move_target() -> Option<(u64, u64)> {
    for _ in 0..4 {
        let before = MOVE_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }

        let active = MOVE_ACTIVE.load(Ordering::Relaxed);
        let x = MOVE_X.load(Ordering::Relaxed);
        let y = MOVE_Y.load(Ordering::Relaxed);
        let after = MOVE_VERSION.load(Ordering::Acquire);

        if before == after && after & 1 == 0 {
            return active.then_some((x, y));
        }
    }

    None
}

/// Returns a manual move only for the selected athlete after an RMB target has been published.
pub fn manual_input_for(athlete_id: usize, tick: u64) -> Option<InputV1> {
    if selected_athlete() != Some(athlete_id) {
        return None;
    }

    let (x, y) = move_target()?;
    MANUAL_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
    LAST_MANUAL_TICK.store(tick, Ordering::Relaxed);
    Some(InputV1::move_to(x, y))
}

pub fn diagnostics() -> ControlDiagnostics {
    let last_tick = LAST_MANUAL_TICK.load(Ordering::Acquire);
    ControlDiagnostics {
        selected_athlete: selected_athlete(),
        move_target: move_target(),
        select_count: SELECT_COUNT.load(Ordering::Acquire),
        move_command_count: MOVE_COMMAND_COUNT.load(Ordering::Acquire),
        manual_input_returns: MANUAL_INPUT_RETURNS.load(Ordering::Acquire),
        last_manual_tick: (last_tick != NO_TICK).then_some(last_tick),
    }
}
