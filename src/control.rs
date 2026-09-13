//! Cross-thread manual-control state for the paced Candidate-A simulation.
//!
//! The client/render thread publishes only physical-control intent: selected athlete identity and
//! RMB simulation coordinates. The Candidate-A `StablePlayerAi` callback resolves each new RMB point
//! against its own `StableSim` snapshot, then persists either a ground MoveTo or an exact entity-id
//! attack intent. Entity identity never comes from the presentation thread.
//!
//! Important stable-API semantic: returning `None` from `StablePlayerAi::think` keeps the built-in
//! input. A manually selected athlete therefore always receives a concrete manual input whenever we
//! can identify its current/last-known position. When no user command is active, that input is a
//! MoveTo to the athlete's own position (neutral hold), not a handoff back to vanilla AI.
//!
//! Attack intent is deliberately split into pursuit and execution. We only emit Attack(Target) when
//! `StableAiContext::is_valid_input` accepts that exact attack. If the attack is ready but invalid
//! (normally because the target is out of range), we chase the target with literal MoveTo instead of
//! asking the game's higher-level attack behavior to decide how to approach it.

mod entity_picker;
mod skill_targeting;

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use entity_picker::pick_hostile_entity;
use mod_api_stable::{
    InputKindV1, InputTargetKindV1, InputTargetV1, InputV1, StableAiContext,
};

pub use skill_targeting::{SkillPreviewMode, SkillSlot, SkillTargetingSnapshot};

const NO_ATHLETE: usize = usize::MAX;
const NO_TARGET: usize = usize::MAX;
const NO_TICK: u64 = u64::MAX;

const COMMAND_NONE: u8 = 0;
const COMMAND_MOVE: u8 = 1;
const COMMAND_ATTACK: u8 = 2;
const COMMAND_RETURN: u8 = 3;
const COMMAND_HOLD: u8 = 4;

// Exact entity collision geometry first. Screen-pixel click forgiveness remains a later polish item.
const MINIMUM_PICK_RADIUS_SIM: u64 = 0;
// There is no native Stop input. When H interrupts Return we issue one movement tick toward the map
// center, then anchor Hold on the following tick. The destination can be far away because it is only
// emitted once; actual displacement is bounded to a single simulation tick.
const HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM: u64 = 32_000;
const DEFAULT_MAP_MAX_SIM: u64 = 960_000;

static SELECTED_ATHLETE: AtomicUsize = AtomicUsize::new(NO_ATHLETE);

// Latest RMB point published by the render thread. VERSION is a tiny seqlock so x/y are coherent.
static RMB_ACTIVE: AtomicBool = AtomicBool::new(false);
static RMB_X: AtomicU64 = AtomicU64::new(0);
static RMB_Y: AtomicU64 = AtomicU64::new(0);
static RMB_VERSION: AtomicU64 = AtomicU64::new(0);
static RESOLVED_RMB_VERSION: AtomicU64 = AtomicU64::new(0);

// Persistent command selected by the simulation thread from the latest explicit user order.
static ACTIVE_COMMAND_KIND: AtomicU8 = AtomicU8::new(COMMAND_NONE);
static ACTIVE_MOVE_X: AtomicU64 = AtomicU64::new(0);
static ACTIVE_MOVE_Y: AtomicU64 = AtomicU64::new(0);
static ACTIVE_ATTACK_TARGET: AtomicUsize = AtomicUsize::new(NO_TARGET);
static HOLD_ANCHOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static HOLD_BREAK_RETURN_PENDING: AtomicBool = AtomicBool::new(false);

// Last known selected-champion position. This lets us keep emitting a concrete neutral input even
// if one callback temporarily cannot expose StableSim after manual ownership has already begun.
static LAST_SELF_POSITION_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_SELF_X: AtomicU64 = AtomicU64::new(0);
static LAST_SELF_Y: AtomicU64 = AtomicU64::new(0);

static SELECT_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_COMMAND_COUNT: AtomicU64 = AtomicU64::new(0); // physical RMB requests
static RMB_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static ATTACK_RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static RETURN_COMMAND_COUNT: AtomicU64 = AtomicU64::new(0);
static MANUAL_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static ATTACK_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static HOLD_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
static CHASE_INPUT_RETURNS: AtomicU64 = AtomicU64::new(0);
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
    pub returning: bool,
    pub select_count: u64,
    /// Physical RMB requests; name retained for compatibility with the current overlay.
    pub move_command_count: u64,
    pub rmb_resolve_count: u64,
    pub move_resolve_count: u64,
    pub attack_resolve_count: u64,
    pub return_command_count: u64,
    pub manual_input_returns: u64,
    pub attack_input_returns: u64,
    pub hold_input_returns: u64,
    pub chase_input_returns: u64,
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
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
}

fn set_active_move(x: u64, y: u64) {
    // Publish kind last: readers that observe MOVE must also observe the matching coordinates.
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(x, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(y, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    ACTIVE_COMMAND_KIND.store(COMMAND_MOVE, Ordering::Release);
}

fn set_active_attack(target_id: usize) {
    // Publish kind last for the same reason as set_active_move().
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_ATTACK_TARGET.store(target_id, Ordering::Relaxed);
    ACTIVE_MOVE_X.store(0, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(0, Ordering::Relaxed);
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    ACTIVE_COMMAND_KIND.store(COMMAND_ATTACK, Ordering::Release);
}

fn set_active_return() {
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(0, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(0, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    ACTIVE_COMMAND_KIND.store(COMMAND_RETURN, Ordering::Release);
}

fn set_active_hold() {
    // H is a real persistent command, not merely "no order". The authoritative simulation callback
    // captures the champion's position on its next tick and keeps that fixed anchor until replaced.
    // A zero-distance MoveTo does not cancel an active Return channel, so remember whether this Hold
    // replaced Return and inject one ordinary movement tick before establishing the fixed anchor.
    let break_return = ACTIVE_COMMAND_KIND.load(Ordering::Acquire) == COMMAND_RETURN;
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(0, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(0, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(break_return, Ordering::Release);
    ACTIVE_COMMAND_KIND.store(COMMAND_HOLD, Ordering::Release);
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

fn hold_recall_cancel_target(from: (u64, u64)) -> (u64, u64) {
    let center = DEFAULT_MAP_MAX_SIM / 2;
    let x = if from.0 < center {
        from.0
            .saturating_add(HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM)
            .min(DEFAULT_MAP_MAX_SIM)
    } else {
        from.0.saturating_sub(HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM)
    };
    let y = if x == from.0 {
        if from.1 < center {
            from.1
                .saturating_add(HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM)
                .min(DEFAULT_MAP_MAX_SIM)
        } else {
            from.1.saturating_sub(HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM)
        }
    } else {
        from.1
    };
    (x, y)
}

pub fn reset() {
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    clear_last_self_position();
    skill_targeting::reset();
    SELECT_COUNT.store(0, Ordering::Release);
    MOVE_COMMAND_COUNT.store(0, Ordering::Release);
    RMB_RESOLVE_COUNT.store(0, Ordering::Release);
    MOVE_RESOLVE_COUNT.store(0, Ordering::Release);
    ATTACK_RESOLVE_COUNT.store(0, Ordering::Release);
    RETURN_COMMAND_COUNT.store(0, Ordering::Release);
    MANUAL_INPUT_RETURNS.store(0, Ordering::Release);
    ATTACK_INPUT_RETURNS.store(0, Ordering::Release);
    HOLD_INPUT_RETURNS.store(0, Ordering::Release);
    CHASE_INPUT_RETURNS.store(0, Ordering::Release);
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
    // A newly selected athlete must never inherit the previous athlete's move, attack, recall, or skill aim.
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    clear_move_target();
    clear_last_self_position();
    skill_targeting::on_selection_changed();
    SELECTED_ATHLETE.store(athlete_id, Ordering::Release);
    SELECT_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn arm_skill(slot: SkillSlot) {
    skill_targeting::arm(slot);
}

pub fn cancel_skill_targeting() {
    skill_targeting::cancel();
}

pub fn skill_targeting_active() -> bool {
    skill_targeting::is_active()
}

pub fn publish_skill_cursor(x: u64, y: u64) {
    skill_targeting::publish_cursor(x, y);
}

pub fn clear_skill_cursor() {
    skill_targeting::clear_cursor();
}

pub fn confirm_skill(x: u64, y: u64) {
    skill_targeting::confirm(x, y);
}

pub fn skill_targeting_snapshot() -> SkillTargetingSnapshot {
    skill_targeting::snapshot()
}

pub fn clamp_skill_target_to_range(
    from: (u64, u64),
    to: (u64, u64),
    range: u64,
) -> (u64, u64) {
    skill_targeting::clamp_to_range(from, to, range)
}

/// B/recall is a persistent explicit manual order. It replaces movement/attack and cancels any
/// armed skill. RMB or a later successful skill cast can interrupt it.
pub fn request_return_home() {
    clear_move_target();
    skill_targeting::cancel();
    set_active_return();
    RETURN_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// H is an explicit persistent stop. If it replaces Return, active_manual_input emits one movement
/// tick first because a zero-distance hold does not interrupt TFM2's recall channel.
pub fn request_hold() {
    skill_targeting::cancel();
    clear_move_target();
}

/// Compatibility entry point used by the render-thread RMB code.
///
/// The point is an unresolved contextual RMB request. The paced simulation callback decides whether
/// it means Attack(entity) or MoveTo(point).
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
    set_active_hold();
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

fn neutral_hold_input(self_position: Option<(u64, u64)>) -> Option<InputV1> {
    let (x, y) = self_position.or_else(last_self_position)?;
    HOLD_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
    Some(InputV1::move_to(x, y))
}

fn active_manual_input(
    ctx: &mut StableAiContext<'_>,
    self_position: Option<(u64, u64)>,
) -> Option<InputV1> {
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
            let player_id = ctx.player_id();

            // Copy every value needed from StableSim, then release that borrow before calling
            // ctx.is_valid_input(). This keeps the stable wrapper borrowing rules simple.
            let (target_x, target_y, attack_cooldown) = {
                let Some(sim) = ctx.sim() else {
                    // Preserve target identity; caller converts this callback into neutral hold.
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
                    // Deliberate fog-of-war rule: losing legal vision breaks target tracking. We do
                    // not auto-reacquire if the same unit later reappears.
                    drop_attack_as_not_visible();
                    return None;
                }

                let (target_x, target_y) = target.pos();
                let attack_cooldown = sim
                    .get_player(player_id)
                    .and_then(|player| player.cooldowns())
                    .map(|cooldowns| cooldowns.0);
                (target_x, target_y, attack_cooldown)
            };

            let attack = InputV1::action(
                InputKindV1::Attack,
                InputTargetV1 {
                    kind: InputTargetKindV1::Target.code(),
                    target_id,
                    ..Default::default()
                },
            );

            if ctx.is_valid_input(&attack) {
                ATTACK_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
                return Some(attack);
            }

            if attack_cooldown == Some(0) || attack_cooldown.is_none() {
                // Attack is ready but TFM2 rejects this exact target action. For the ordinary chase
                // case this means range: preserve the target id, but approach with literal movement
                // rather than letting Attack(Target) invoke built-in retreat/recall/threat logic.
                CHASE_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
                return Some(InputV1::move_to(target_x, target_y));
            }

            // During the basic-attack recovery window, do not walk a ranged champion all the way
            // into the target merely because is_valid_input also checks cooldown. Hold until the
            // attack is ready; then either attack if legal or resume pursuit if range was lost.
            neutral_hold_input(self_position)
        }
        COMMAND_RETURN => Some(InputV1::return_home()),
        COMMAND_HOLD => {
            if HOLD_BREAK_RETURN_PENDING.swap(false, Ordering::AcqRel) {
                let from = self_position.or_else(last_self_position)?;
                let (x, y) = hold_recall_cancel_target(from);
                HOLD_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
                return Some(InputV1::move_to(x, y));
            }

            if !HOLD_ANCHOR_ACTIVE.load(Ordering::Acquire) {
                let (x, y) = self_position.or_else(last_self_position)?;
                ACTIVE_MOVE_X.store(x, Ordering::Relaxed);
                ACTIVE_MOVE_Y.store(y, Ordering::Relaxed);
                HOLD_ANCHOR_ACTIVE.store(true, Ordering::Release);
            }
            HOLD_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
            Some(InputV1::move_to(
                ACTIVE_MOVE_X.load(Ordering::Relaxed),
                ACTIVE_MOVE_Y.load(Ordering::Relaxed),
            ))
        }
        _ => None,
    }
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

    // An armed skill does not erase the persistent RMB order. Only a successful LMB confirmation
    // preempts it for this simulation tick; the move/attack intent can resume on the next tick.
    // Return is different: a successful skill cast intentionally interrupts recall instead of
    // silently resuming it one tick later.
    if let Some(input) = skill_targeting::manual_skill_input(ctx, self_position) {
        if ACTIVE_COMMAND_KIND.load(Ordering::Acquire) == COMMAND_RETURN {
            clear_active_command();
        }
        MANUAL_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
        LAST_MANUAL_TICK.store(tick, Ordering::Relaxed);
        return Some(input);
    }

    let input = match active_manual_input(ctx, self_position) {
        Some(input) => input,
        None => neutral_hold_input(self_position)?,
    };

    MANUAL_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
    LAST_MANUAL_TICK.store(tick, Ordering::Relaxed);
    Some(input)
}

pub fn diagnostics() -> ControlDiagnostics {
    let last_tick = LAST_MANUAL_TICK.load(Ordering::Acquire);
    let active_kind = ACTIVE_COMMAND_KIND.load(Ordering::Acquire);
    let attack_target = match active_kind {
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
        returning: active_kind == COMMAND_RETURN,
        select_count: SELECT_COUNT.load(Ordering::Acquire),
        move_command_count: MOVE_COMMAND_COUNT.load(Ordering::Acquire),
        rmb_resolve_count: RMB_RESOLVE_COUNT.load(Ordering::Acquire),
        move_resolve_count: MOVE_RESOLVE_COUNT.load(Ordering::Acquire),
        attack_resolve_count: ATTACK_RESOLVE_COUNT.load(Ordering::Acquire),
        return_command_count: RETURN_COMMAND_COUNT.load(Ordering::Acquire),
        manual_input_returns: MANUAL_INPUT_RETURNS.load(Ordering::Acquire),
        attack_input_returns: ATTACK_INPUT_RETURNS.load(Ordering::Acquire),
        hold_input_returns: HOLD_INPUT_RETURNS.load(Ordering::Acquire),
        chase_input_returns: CHASE_INPUT_RETURNS.load(Ordering::Acquire),
        target_drop_count: TARGET_DROP_COUNT.load(Ordering::Acquire),
        target_drop_vision_count: TARGET_DROP_VISION_COUNT.load(Ordering::Acquire),
        target_drop_dead_count: TARGET_DROP_DEAD_COUNT.load(Ordering::Acquire),
        target_drop_invalid_count: TARGET_DROP_INVALID_COUNT.load(Ordering::Acquire),
        last_manual_tick: (last_tick != NO_TICK).then_some(last_tick),
    }
}
