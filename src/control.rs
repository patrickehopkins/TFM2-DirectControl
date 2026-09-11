//! Cross-thread manual-control state for the paced Candidate-A simulation.
//!
//! The client/render thread publishes only physical-control intent: selected athlete identity and
//! RMB simulation coordinates. The Candidate-A `StablePlayerAi` callback resolves each new RMB point
//! against its own `StableSim` snapshot, then persists either a ground MoveTo or an exact entity-id
//! Attack command. This keeps entity identity out of the presentation thread.
//!
//! Important stable-API semantic: returning `None` from `StablePlayerAi::think` keeps the built-in
//! input. A manually selected athlete therefore always receives a concrete manual input whenever we
//! can identify its current/last-known position. When no user command is active, that input is a
//! MoveTo to the athlete's own position (neutral hold), not a handoff back to vanilla AI.

mod entity_picker;

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use entity_picker::pick_hostile_entity;
use mod_api_stable::{
    InputKindV1, InputTargetKindV1, InputTargetV1, InputV1, StableAiContext,
};

const NO_ATHLETE: usize = usize::MAX;
const NO_TARGET: usize = usize::MAX;
const NO_TICK: u64 = u64::MAX;

const COMMAND_NONE: u8 = 0;
const COMMAND_MOVE: u8 = 1;
const COMMAND_ATTACK: u8 = 2;

// Stage 6A deliberately starts with the entity's own collision radius as the click shape. Once
// exact-target behavior is physically proven, add zoom-independent screen-pixel forgiveness at
// the render->simulation boundary rather than baking an arbitrary world-space constant here.
const MINIMUM_PICK_RADIUS_SIM: u64 = 0;

static SELECTED_ATHLETE: AtomicUsize = AtomicUsize::new(NO_ATHLETE);

// Latest RMB point published by the render thread. VERSION is a tiny seqlock so x/y are coherent.
static RMB_ACTIVE: AtomicBool = AtomicBool::new(false);
static RMB_X: AtomicU64 = AtomicU64::new(0);
static RMB_Y: AtomicU64 = AtomicU64::new(0);
static RMB_VERSION: AtomicU64 = AtomicU64::new(0);
static RESOLVED_RMB_VERSION: AtomicU64 = AtomicU64::new(0);

// Persistent command selected by the simulation thread from the latest RMB request.
static ACTIVE_COMMAND_KIND: AtomicU8 = AtomicU8::new(COMMAND_NONE);
static ACTIVE_MOVE_X: AtomicU64 = AtomicU64::new(0);
static ACTIVE_MOVE_Y: AtomicU64 = AtomicU64::new(0);
static ACTIVE_ATTACK_TARGET: AtomicUsize = AtomicUsize::new(NO_TARGET);

// Last known selected-champion position. This lets us keep emitting a concrete neutral input even
// if one callback temporarily cannot expose StableSim after manual ownership has already begun.
static LAST_SELF_POSITION_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_SELF_X: AtomicU64 = AtomicU64::new(0);
static LAST_SELF_Y: AtomicU64 = AtomicU64::new(0);

static SELECT_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_COMMAND_COUNT: AtomicU64 = AtomicU64::new(0); // retained name: counts physical RMB requests
static RMB_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static ATTACK_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static MANUAL_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static ATTACK_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static HOLD_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static TARGET_DROP_COUNT: AtomicU64 = AtomicU64::new(0);
static TARGET_DROP_VISION_COUNT: AtomicU64 = AtomicU64::new(0);
static TARGET_DROP_DEAD_COUNT: AtomicU64 = AtomicU64::new(0);
static TARGET_DROP_INVALID_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_MANUAL_TICK: AtomicU64 = AtomicU64::new(NO_TICK);

#[derive(Debug, Clone, Copy)]
pub struct ControlDiagnostics {
    pub selected_athlete: Option<usize>,
    /// Latest RMB simulation point. Kept under the old name so the existing overlay stays useful.
    pub move_target: Option<(u64, u64)>,
    pub attack_target: Option<usize>,
    pub select_count: u64,
    /// Physical RMB requests; name retained for compatibility with the current overlay.
    pub move_command_count: u64,
    pub rmb_resolve_count: u64,
    pub move_resolve_count: u64,
    pub attack_resolve_count: u64,
    pub manual_input_returns: u64,
    pub attack_input_returns: u64,
    pub hold_input_returns: u64,
    pub target_drop_count: u64,
    pub target_drop_vision_count: u64,
    pub target_drop_dead_count: u64,
    pub target_drop_invalid_count: u64,
    pub last_manual_tick: Option<u64>,
}

fn clear_active_command() {
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(0, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(0, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
}

fn set_active_move(x: u64, y: u64) {
    // Publish kind last: readers that observe MOVE must also observe the matching coordinates.
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(x, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(y, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
    ACTIVE_COMMAND_KIND.store(COMMAND_MOVE, Ordering::Release);
}

fn set_active_attack(target_id: usize) {
    // Publish kind last for the same reason as set_active_move().
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_ATTACK_TARGET.store(target_id, Ordering::Relaxed);
    ACTIVE_MOVE_X.store(0, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(0, Ordering::Relaxed);
    ACTIVE_COMMAND_KIND.store(COMMAND_ATTACK, Ordering::Release);
}

fn clear_last_self_position() {
    LAST_SELF_POSITION_ACTIVE.store(false, Ordering::Release);
    LAST_SELF_X.store(0, Ordering::Relaxed);
    LAST_SELF_Y.store(0, Ordering::Relaxed);
}

fn remember_self_position(x: u64, y: u64) {
    LAST_SELF_X.store(x, Ordering::Relaxed);
    LAST_SELF_Y.store(y, Ordering::Relaxed);
    LAST_SELF_POSITION_ACTIVE.store(true, Ordering::Release);
}

fn last_self_position() -> Option<(u64, u64)> {
    if !LAST_SELF_POSITION_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    Some((
        LAST_SELF_X.load(Ordering::Relaxed),
        LAST_SELF_Y.load(Ordering::Relaxed),
    ))
}

pub fn reset() {
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    clear_last_self_position();
    SELECT_COUNT.store(0, Ordering::Release);
    MOVE_COMMAND_COUNT.store(0, Ordering::Release);
    RMB_RESOLVE_COUNT.store(0, Ordering::Release);
    MOVE_RESOLVE_COUNT.store(0, Ordering::Release);
    ATTACK_RESOLVE_COUNT.store(0, Ordering::Release);
    MANUAL_INPUT_RETURNS.store(0, Ordering::Release);
    ATTACK_INPUT_RETURNS.store(0, Ordering::Release);
    HOLD_INPUT_RETURNS.store(0, Ordering::Release);
    TARGET_DROP_COUNT.store(0, Ordering::Release);
    TARGET_DROP_VISION_COUNT.store(0, Ordering::Release);
    TARGET_DROP_DEAD_COUNT.store(0, Ordering::Release);
    TARGET_DROP_INVALID_COUNT.store(0, Ordering::Release);
    LAST_MANUAL_TICK.store(NO_TICK, Ordering::Release);
}

pub fn selected_athlete() -> Option<usize> {
    match SELECTED_ATHLETE.load(Ordering::Acquire) {
        NO_ATHLETE => None,
        athlete_id => Some(athlete_id),
    }
}

pub fn select_athlete(athlete_id: usize) {
    // A newly selected athlete must never inherit the previous athlete's move, attack, or hold point.
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    clear_last_self_position();
    SELECTED_ATHLETE.store(athlete_id, Ordering::Release);
    SELECT_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Compatibility entry point used by the current render-thread RMB code.
///
/// The point is no longer assumed to be a move destination. It is an unresolved contextual RMB
/// request; the paced simulation callback decides whether it means Attack(entity) or MoveTo(point).
pub fn publish_move_target(x: u64, y: u64) {
    RMB_VERSION.fetch_add(1, Ordering::AcqRel); // odd = write in progress
    RMB_X.store(x, Ordering::Relaxed);
    RMB_Y.store(y, Ordering::Relaxed);
    RMB_ACTIVE.store(true, Ordering::Relaxed);
    RMB_VERSION.fetch_add(1, Ordering::Release); // even = stable snapshot/new request id
    MOVE_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn clear_move_target() {
    RMB_VERSION.fetch_add(1, Ordering::AcqRel);
    RMB_ACTIVE.store(false, Ordering::Relaxed);
    RMB_X.store(0, Ordering::Relaxed);
    RMB_Y.store(0, Ordering::Relaxed);
    let stable_version = RMB_VERSION.fetch_add(1, Ordering::Release) + 1;
    RESOLVED_RMB_VERSION.store(stable_version, Ordering::Release);
    clear_active_command();
}

fn rmb_request() -> Option<(u64, u64, u64)> {
    for _ in 0..4 {
        let before = RMB_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }

        let active = RMB_ACTIVE.load(Ordering::Relaxed);
        let x = RMB_X.load(Ordering::Relaxed);
        let y = RMB_Y.load(Ordering::Relaxed);
        let after = RMB_VERSION.load(Ordering::Acquire);

        if before == after && after & 1 == 0 {
            return active.then_some((x, y, after));
        }
    }

    None
}

pub fn move_target() -> Option<(u64, u64)> {
    rmb_request().map(|(x, y, _)| (x, y))
}

fn current_champion_position(ctx: &mut StableAiContext<'_>) -> Option<(u64, u64)> {
    let player_id = ctx.player_id();
    let sim = ctx.sim()?;
    let player = sim.get_player(player_id)?;
    let champion = player.champion()?;
    Some(champion.pos())
}

fn resolve_latest_rmb(ctx: &mut StableAiContext<'_>) {
    let Some((x, y, request_version)) = rmb_request() else {
        return;
    };

    if RESOLVED_RMB_VERSION.load(Ordering::Acquire) == request_version {
        return;
    }

    let controlled_team = ctx.team();
    let Some(sim) = ctx.sim() else {
        // Fail safe: do not consume the request until authoritative simulation state is available.
        return;
    };

    if let Some(picked) = pick_hostile_entity(
        &sim,
        controlled_team,
        x,
        y,
        MINIMUM_PICK_RADIUS_SIM,
    ) {
        set_active_attack(picked.id);
        ATTACK_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        set_active_move(x, y);
        MOVE_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    RMB_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    RESOLVED_RMB_VERSION.store(request_version, Ordering::Release);
}

fn drop_attack_as_dead() {
    TARGET_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    TARGET_DROP_DEAD_COUNT.fetch_add(1, Ordering::Relaxed);
    clear_active_command();
}

fn drop_attack_as_not_visible() {
    TARGET_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    TARGET_DROP_VISION_COUNT.fetch_add(1, Ordering::Relaxed);
    clear_active_command();
}

fn drop_attack_as_invalid() {
    TARGET_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    TARGET_DROP_INVALID_COUNT.fetch_add(1, Ordering::Relaxed);
    clear_active_command();
}

fn active_manual_input(ctx: &mut StableAiContext<'_>) -> Option<InputV1> {
    match ACTIVE_COMMAND_KIND.load(Ordering::Acquire) {
        COMMAND_MOVE => Some(InputV1::move_to(
            ACTIVE_MOVE_X.load(Ordering::Relaxed),
            ACTIVE_MOVE_Y.load(Ordering::Relaxed),
        )),
        COMMAND_ATTACK => {
            let target_id = ACTIVE_ATTACK_TARGET.load(Ordering::Relaxed);
            if target_id == NO_TARGET {
                drop_attack_as_invalid();
                return None;
            }

            let controlled_team = ctx.team();
            let Some(sim) = ctx.sim() else {
                // Keep the target identity and use neutral hold for this callback. If the simulation
                // view comes back next tick, the exact attack can resume without guessing a target.
                return None;
            };
            let Some(target) = sim.get_entity(target_id) else {
                drop_attack_as_invalid();
                return None;
            };

            if !target.is_alive() {
                drop_attack_as_dead();
                return None;
            }
            if !target.is_targetable() || target.team() == controlled_team {
                drop_attack_as_invalid();
                return None;
            }
            if !sim.is_visible(controlled_team, target_id) {
                // Deliberate fog-of-war rule: losing legal vision breaks target tracking. We do not
                // auto-reacquire if the same unit later reappears; the user must issue another RMB.
                drop_attack_as_not_visible();
                return None;
            }

            ATTACK_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
            Some(InputV1::action(
                InputKindV1::Attack,
                InputTargetV1 {
                    kind: InputTargetKindV1::Target.code(),
                    target_id,
                    ..Default::default()
                },
            ))
        }
        _ => None,
    }
}

fn neutral_hold_input(self_position: Option<(u64, u64)>) -> Option<InputV1> {
    let (x, y) = self_position.or_else(last_self_position)?;
    HOLD_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
    Some(InputV1::move_to(x, y))
}

/// Returns the persistent manual command for the selected athlete.
///
/// A newly published RMB point is resolved exactly once against this callback's `StableSim` state.
/// Subsequent ticks retain the resulting entity id or ground destination until the user clicks again,
/// changes controlled athlete, leaves the match, or the attack target becomes invalid.
///
/// Target invalidation never returns control to vanilla AI. If a tracked target dies, becomes
/// untargetable, changes team, or leaves vision, the target is forgotten and the selected champion
/// receives a neutral hold input until the user's next explicit command.
pub fn manual_input_for(ctx: &mut StableAiContext<'_>, tick: u64) -> Option<InputV1> {
    if selected_athlete() != Some(ctx.athlete_id()) {
        return None;
    }

    let self_position = current_champion_position(ctx);
    if let Some((x, y)) = self_position {
        remember_self_position(x, y);
    }

    resolve_latest_rmb(ctx);
    let input = match active_manual_input(ctx) {
        Some(input) => input,
        None => neutral_hold_input(self_position)?,
    };

    MANUAL_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
    LAST_MANUAL_TICK.store(tick, Ordering::Relaxed);
    Some(input)
}

pub fn diagnostics() -> ControlDiagnostics {
    let last_tick = LAST_MANUAL_TICK.load(Ordering::Acquire);
    let attack_target = match ACTIVE_COMMAND_KIND.load(Ordering::Acquire) {
        COMMAND_ATTACK => match ACTIVE_ATTACK_TARGET.load(Ordering::Acquire) {
            NO_TARGET => None,
            target => Some(target),
        },
        _ => None,
    };

    ControlDiagnostics {
        selected_athlete: selected_athlete(),
        move_target: move_target(),
        attack_target,
        select_count: SELECT_COUNT.load(Ordering::Acquire),
        move_command_count: MOVE_COMMAND_COUNT.load(Ordering::Acquire),
        rmb_resolve_count: RMB_RESOLVE_COUNT.load(Ordering::Acquire),
        move_resolve_count: MOVE_RESOLVE_COUNT.load(Ordering::Acquire),
        attack_resolve_count: ATTACK_RESOLVE_COUNT.load(Ordering::Acquire),
        manual_input_returns: MANUAL_INPUT_RETURNS.load(Ordering::Acquire),
        attack_input_returns: ATTACK_INPUT_RETURNS.load(Ordering::Acquire),
        hold_input_returns: HOLD_INPUT_RETURNS.load(Ordering::Acquire),
        target_drop_count: TARGET_DROP_COUNT.load(Ordering::Acquire),
        target_drop_vision_count: TARGET_DROP_VISION_COUNT.load(Ordering::Acquire),
        target_drop_dead_count: TARGET_DROP_DEAD_COUNT.load(Ordering::Acquire),
        target_drop_invalid_count: TARGET_DROP_INVALID_COUNT.load(Ordering::Acquire),
        last_manual_tick: (last_tick != NO_TICK).then_some(last_tick),
    }
}
