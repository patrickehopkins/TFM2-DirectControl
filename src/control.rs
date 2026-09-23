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
//! `StableAiContext::is_valid_input` accepts that exact attack. When Attack is temporarily invalid,
//! we ask the same validator whether literal MoveTo(target) is legal on that exact frame. If movement
//! is legal we chase; otherwise we hold. This lets the game's own action/cancel rules define recovery
//! timing instead of inventing a cooldown-based delay.

mod entity_picker;
mod skill_targeting;

use std::sync::{
    atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    Mutex, OnceLock,
};

use entity_picker::{effective_pick_radius, pick_hostile_entity, visible_targetable_entities};
use mod_api_stable::{
    InputKindV1, InputTargetKindV1, InputTargetV1, InputV1, StableAiContext,
};

pub use entity_picker::{ClickableEntityGeometry, EntityKind};
pub use skill_targeting::{SkillPreviewMode, SkillSlot, SkillTargetingSnapshot};

const NO_ATHLETE: usize = usize::MAX;
const NO_TEAM: usize = usize::MAX;
const NO_TARGET: usize = usize::MAX;
const NO_TICK: u64 = u64::MAX;
const CLICK_TARGET_OVERLAY_REFRESH_TICKS: u64 = 6;

const COMMAND_NONE: u8 = 0;
const COMMAND_MOVE: u8 = 1;
const COMMAND_ATTACK: u8 = 2;
const COMMAND_RETURN: u8 = 3;
const COMMAND_HOLD: u8 = 4;
const COMMAND_ATTACK_MOVE: u8 = 5;

// Battlefield RMB now carries live camera scale so the picker can add screen-pixel forgiveness.
// There is no native Stop input. When H interrupts Return we issue one movement tick toward the map
// center, then anchor Hold on the following tick. The destination can be far away because it is only
// emitted once; actual displacement is bounded to a single simulation tick.
const HOLD_RECALL_CANCEL_TARGET_OFFSET_SIM: u64 = 32_000;
const DEFAULT_MAP_MAX_SIM: u64 = 960_000;

static SELECTED_ATHLETE: AtomicUsize = AtomicUsize::new(NO_ATHLETE);
// Published by the authoritative simulation callback for the currently selected athlete.
// The render thread uses this only for spectator fog/camera-side selection; gameplay target
// legality still comes from StableAiContext/StableSim.
static SELECTED_TEAM: AtomicUsize = AtomicUsize::new(NO_TEAM);
static ATTACK_MOVE_ARMED: AtomicBool = AtomicBool::new(false);

// Latest RMB point published by the render thread. VERSION is a tiny seqlock so x/y are coherent.
static RMB_ACTIVE: AtomicBool = AtomicBool::new(false);
static RMB_X: AtomicU64 = AtomicU64::new(0);
static RMB_Y: AtomicU64 = AtomicU64::new(0);
static RMB_SIM_UNITS_PER_PX: AtomicU64 = AtomicU64::new(0);
static RMB_VERSION: AtomicU64 = AtomicU64::new(0);
static RESOLVED_RMB_VERSION: AtomicU64 = AtomicU64::new(0);

// Persistent command selected by the simulation thread from the latest explicit user order.
// ATTACK_MOVE reuses ACTIVE_MOVE_X/Y as its destination and ACTIVE_ATTACK_TARGET as its currently
// acquired hostile. Losing that hostile does not erase the destination.
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
static LAST_CLICK_TARGET_OVERLAY_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static CLICK_TARGET_OVERLAY: OnceLock<Mutex<Vec<ClickableEntityGeometry>>> = OnceLock::new();

fn click_target_overlay_storage() -> &'static Mutex<Vec<ClickableEntityGeometry>> {
    CLICK_TARGET_OVERLAY.get_or_init(|| Mutex::new(Vec::new()))
}

fn clear_click_target_overlay() {
    if let Ok(mut snapshot) = click_target_overlay_storage().lock() {
        snapshot.clear();
    }
}

pub fn refresh_click_target_overlay(ctx: &mut StableAiContext<'_>, tick: u64) {
    let last = LAST_CLICK_TARGET_OVERLAY_TICK.load(Ordering::Acquire);
    if last != NO_TICK && tick >= last && tick - last < CLICK_TARGET_OVERLAY_REFRESH_TICKS {
        return;
    }

    let Some(controlled_team) = selected_team() else {
        return;
    };
    let Some(sim) = ctx.sim() else {
        return;
    };
    let next = visible_targetable_entities(&sim, controlled_team);
    if let Ok(mut snapshot) = click_target_overlay_storage().lock() {
        *snapshot = next;
        LAST_CLICK_TARGET_OVERLAY_TICK.store(tick, Ordering::Release);
    }
}

pub fn click_target_overlay_snapshot() -> Vec<ClickableEntityGeometry> {
    click_target_overlay_storage()
        .lock()
        .map(|snapshot| snapshot.clone())
        .unwrap_or_default()
}

pub fn click_target_effective_radius(
    kind: EntityKind,
    collision_radius: usize,
    sim_units_per_px: u64,
) -> u64 {
    effective_pick_radius(kind, collision_radius, sim_units_per_px)
}

#[derive(Debug, Clone, Copy)]
pub struct ControlDiagnostics {
    pub selected_athlete: Option<usize>,
    /// Latest RMB simulation point. Kept under the old name so the existing overlay stays useful.
    pub move_target: Option<(u64, u64)>,
    pub attack_target: Option<usize>,
    pub returning: bool,
    pub attack_moving: bool,
    pub attack_move_destination: Option<(u64, u64)>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedTargetState {
    Valid((u64, u64)),
    Lost,
    StateUnavailable,
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

fn set_active_attack_move(x: u64, y: u64) {
    ACTIVE_COMMAND_KIND.store(COMMAND_NONE, Ordering::Release);
    ACTIVE_MOVE_X.store(x, Ordering::Relaxed);
    ACTIVE_MOVE_Y.store(y, Ordering::Relaxed);
    ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Relaxed);
    HOLD_ANCHOR_ACTIVE.store(false, Ordering::Release);
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    ACTIVE_COMMAND_KIND.store(COMMAND_ATTACK_MOVE, Ordering::Release);
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
    SELECTED_TEAM.store(NO_TEAM, Ordering::Release);
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    clear_move_target();
    HOLD_BREAK_RETURN_PENDING.store(false, Ordering::Release);
    clear_last_self_position();
    clear_click_target_overlay();
    LAST_CLICK_TARGET_OVERLAY_TICK.store(NO_TICK, Ordering::Release);
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

pub fn selected_team() -> Option<usize> {
    match SELECTED_TEAM.load(Ordering::Acquire) {
        NO_TEAM => None,
        team => Some(team),
    }
}

pub fn select_athlete(athlete_id: usize) {
    // A newly selected athlete must never inherit the previous athlete's move, attack, recall,
    // attack-move, or skill aim.
    SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    SELECTED_TEAM.store(NO_TEAM, Ordering::Release);
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    clear_move_target();
    clear_last_self_position();
    clear_click_target_overlay();
    LAST_CLICK_TARGET_OVERLAY_TICK.store(NO_TICK, Ordering::Release);
    skill_targeting::on_selection_changed();
    SELECTED_ATHLETE.store(athlete_id, Ordering::Release);
    SELECT_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn arm_skill(slot: SkillSlot) {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
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

pub fn arm_attack_move() {
    skill_targeting::cancel();
    ATTACK_MOVE_ARMED.store(true, Ordering::Release);
}

pub fn cancel_attack_move() {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
}

pub fn attack_move_armed() -> bool {
    ATTACK_MOVE_ARMED.load(Ordering::Acquire)
}

pub fn confirm_attack_move(x: u64, y: u64) {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    clear_rmb_request();
    set_active_attack_move(x, y);
}

/// B/recall is a persistent explicit manual order. It replaces movement/attack and cancels any
/// armed skill or attack-move cursor. RMB or a later successful skill cast can interrupt it.
pub fn request_return_home() {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    clear_move_target();
    skill_targeting::cancel();
    set_active_return();
    RETURN_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// H is an explicit persistent stop. If it replaces Return, active_manual_input emits one movement
/// tick first because a zero-distance hold does not interrupt TFM2's recall channel.
pub fn request_hold() {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    skill_targeting::cancel();
    clear_move_target();
}

/// Compatibility entry point used by the render-thread RMB code.
///
/// The point is an unresolved contextual RMB request. The paced simulation callback decides whether
/// it means Attack(entity) or MoveTo(point).
pub fn publish_move_target(x: u64, y: u64) {
    publish_move_target_with_pick_scale(x, y, 0);
}

/// Publishes an unresolved contextual RMB request together with the current camera scale.
///
/// `sim_units_per_px` is used only to expand click/select geometry in the simulation callback.
/// Passing zero preserves exact collision geometry, which is useful for minimap commands.
pub fn publish_move_target_with_pick_scale(x: u64, y: u64, sim_units_per_px: u64) {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    RMB_VERSION.fetch_add(1, Ordering::AcqRel); // odd = write in progress
    RMB_X.store(x, Ordering::Relaxed);
    RMB_Y.store(y, Ordering::Relaxed);
    RMB_SIM_UNITS_PER_PX.store(sim_units_per_px, Ordering::Relaxed);
    RMB_ACTIVE.store(true, Ordering::Relaxed);
    RMB_VERSION.fetch_add(1, Ordering::Release); // even = stable snapshot/new request id
    MOVE_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
}

fn clear_rmb_request() {
    RMB_VERSION.fetch_add(1, Ordering::AcqRel);
    RMB_ACTIVE.store(false, Ordering::Relaxed);
    RMB_X.store(0, Ordering::Relaxed);
    RMB_Y.store(0, Ordering::Relaxed);
    RMB_SIM_UNITS_PER_PX.store(0, Ordering::Relaxed);
    let stable_version = RMB_VERSION.fetch_add(1, Ordering::Release) + 1;
    RESOLVED_RMB_VERSION.store(stable_version, Ordering::Release);
}

pub fn clear_move_target() {
    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    clear_rmb_request();
    set_active_hold();
}

fn rmb_request() -> Option<(u64, u64, u64, u64)> {
    for _ in 0..4 {
        let before = RMB_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }

        let active = RMB_ACTIVE.load(Ordering::Relaxed);
        let x = RMB_X.load(Ordering::Relaxed);
        let y = RMB_Y.load(Ordering::Relaxed);
        let sim_units_per_px = RMB_SIM_UNITS_PER_PX.load(Ordering::Relaxed);
        let after = RMB_VERSION.load(Ordering::Acquire);

        if before == after && after & 1 == 0 {
            return active.then_some((x, y, sim_units_per_px, after));
        }
    }

    None
}

pub fn move_target() -> Option<(u64, u64)> {
    rmb_request().map(|(x, y, _, _)| (x, y))
}

fn current_champion_position(ctx: &mut StableAiContext<'_>) -> Option<(u64, u64)> {
    let player_id = ctx.player_id();
    let sim = ctx.sim()?;
    let player = sim.get_player(player_id)?;
    let champion = player.champion()?;
    Some(champion.pos())
}

fn resolve_latest_rmb(ctx: &mut StableAiContext<'_>) {
    let Some((x, y, sim_units_per_px, request_version)) = rmb_request() else {
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

    if let Some(picked) =
        pick_hostile_entity(&sim, controlled_team, x, y, sim_units_per_px)
    {
        set_active_attack(picked.id);
        ATTACK_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        set_active_move(x, y);
        MOVE_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    RMB_RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    RESOLVED_RMB_VERSION.store(request_version, Ordering::Release);
}

fn record_target_drop(dead: bool, vision: bool, clear_whole_command: bool) {
    TARGET_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    if dead {
        TARGET_DROP_DEAD_COUNT.fetch_add(1, Ordering::Relaxed);
    } else if vision {
        TARGET_DROP_VISION_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        TARGET_DROP_INVALID_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    if clear_whole_command {
        clear_active_command();
    } else {
        ACTIVE_ATTACK_TARGET.store(NO_TARGET, Ordering::Release);
    }
}

fn tracked_target_position(
    ctx: &mut StableAiContext<'_>,
    target_id: usize,
    clear_whole_command_on_loss: bool,
) -> TrackedTargetState {
    let controlled_team = ctx.team();
    let Some(sim) = ctx.sim() else {
        return TrackedTargetState::StateUnavailable;
    };
    let Some(target) = sim.get_entity(target_id) else {
        record_target_drop(false, false, clear_whole_command_on_loss);
        return TrackedTargetState::Lost;
    };

    if !target.is_alive() {
        record_target_drop(true, false, clear_whole_command_on_loss);
        return TrackedTargetState::Lost;
    }
    if !target.is_targetable() || target.team() == controlled_team {
        record_target_drop(false, false, clear_whole_command_on_loss);
        return TrackedTargetState::Lost;
    }
    if !sim.is_visible(controlled_team, target_id) {
        record_target_drop(false, true, clear_whole_command_on_loss);
        return TrackedTargetState::Lost;
    }

    TrackedTargetState::Valid(target.pos())
}

fn attack_or_chase_input(
    ctx: &mut StableAiContext<'_>,
    self_position: Option<(u64, u64)>,
    target_id: usize,
    target_position: (u64, u64),
) -> Option<InputV1> {
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

    let chase = InputV1::move_to(target_position.0, target_position.1);
    if ctx.is_valid_input(&chase) {
        CHASE_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
        return Some(chase);
    }

    neutral_hold_input(self_position)
}

fn nearest_legal_attack_target(
    ctx: &mut StableAiContext<'_>,
    self_position: Option<(u64, u64)>,
) -> Option<usize> {
    let (self_x, self_y) = self_position?;
    let controlled_team = ctx.team();

    // Gather identities and geometry first so the StableSim borrow ends before is_valid_input().
    let mut candidates: Vec<(u128, usize)> = {
        let sim = ctx.sim()?;
        let mut candidates = Vec::new();
        for index in 0..sim.entity_count() {
            let Some(entity) = sim.entity_at(index) else {
                continue;
            };
            let id = entity.id();
            if !entity.is_alive()
                || !entity.is_targetable()
                || entity.team() == controlled_team
                || !sim.is_visible(controlled_team, id)
            {
                continue;
            }

            let (x, y) = entity.pos();
            let dx = x.abs_diff(self_x) as u128;
            let dy = y.abs_diff(self_y) as u128;
            candidates.push((dx * dx + dy * dy, id));
        }
        candidates
    };

    candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    for (_, target_id) in candidates {
        let attack = InputV1::action(
            InputKindV1::Attack,
            InputTargetV1 {
                kind: InputTargetKindV1::Target.code(),
                target_id,
                ..Default::default()
            },
        );
        if ctx.is_valid_input(&attack) {
            return Some(target_id);
        }
    }

    None
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
                clear_active_command();
                return None;
            }

            match tracked_target_position(ctx, target_id, true) {
                TrackedTargetState::Valid(target_position) => {
                    attack_or_chase_input(ctx, self_position, target_id, target_position)
                }
                TrackedTargetState::Lost | TrackedTargetState::StateUnavailable => None,
            }
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
        COMMAND_ATTACK_MOVE => {
            let retained_target = ACTIVE_ATTACK_TARGET.load(Ordering::Acquire);
            if retained_target != NO_TARGET {
                match tracked_target_position(ctx, retained_target, false) {
                    TrackedTargetState::Valid(target_position) => {
                        return attack_or_chase_input(
                            ctx,
                            self_position,
                            retained_target,
                            target_position,
                        );
                    }
                    TrackedTargetState::StateUnavailable => {
                        return neutral_hold_input(self_position);
                    }
                    TrackedTargetState::Lost => {}
                }
            }

            // Classic attack-move acquisition: do not auto-hunt distant visible enemies. We only
            // acquire a hostile when Attack(Target) itself is legal now, choosing the nearest legal
            // target to the controlled champion. Once acquired, retain it and use the same exact
            // attack/chase timing as an explicit RMB target until it dies or otherwise becomes invalid.
            if let Some(target_id) = nearest_legal_attack_target(ctx, self_position) {
                ACTIVE_ATTACK_TARGET.store(target_id, Ordering::Release);
                let attack = InputV1::action(
                    InputKindV1::Attack,
                    InputTargetV1 {
                        kind: InputTargetKindV1::Target.code(),
                        target_id,
                        ..Default::default()
                    },
                );
                ATTACK_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
                return Some(attack);
            }

            let destination = (
                ACTIVE_MOVE_X.load(Ordering::Relaxed),
                ACTIVE_MOVE_Y.load(Ordering::Relaxed),
            );
            let advance = InputV1::move_to(destination.0, destination.1);
            if ctx.is_valid_input(&advance) {
                CHASE_INPUT_RETURNS.fetch_add(1, Ordering::Relaxed);
                Some(advance)
            } else {
                neutral_hold_input(self_position)
            }
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
/// Target invalidation never returns control to vanilla AI. If a direct tracked target dies, becomes
/// untargetable, changes team, or leaves vision, the target is forgotten and the selected champion
/// receives a neutral hold input until the user's next explicit command. Attack-move differs only in
/// that losing its acquired target resumes the original attack-move destination and permits a new
/// in-range target to be acquired later.
pub fn manual_input_for(ctx: &mut StableAiContext<'_>, tick: u64) -> Option<InputV1> {
    if selected_athlete() != Some(ctx.athlete_id()) {
        return None;
    }

    // Resolve the selected champion's actual simulation side rather than inferring it from
    // F-key position or the user's starting team. This is consumed by spectator fog and the
    // click-target overlay. A newly selected champion publishes its team before the second
    // refresh so the overlay appears immediately rather than waiting for the next tick.
    SELECTED_TEAM.store(ctx.team(), Ordering::Release);

    let self_position = current_champion_position(ctx);
    if let Some((x, y)) = self_position {
        remember_self_position(x, y);
    }

    resolve_latest_rmb(ctx);

    // An armed skill does not erase the persistent RMB/attack-move order. Only a successful LMB
    // confirmation preempts it for this simulation tick; the prior order can resume on the next tick.
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
        COMMAND_ATTACK | COMMAND_ATTACK_MOVE => match ACTIVE_ATTACK_TARGET.load(Ordering::Acquire) {
            NO_TARGET => None,
            target => Some(target),
        },
        _ => None,
    };
    let attack_move_destination = (active_kind == COMMAND_ATTACK_MOVE).then_some((
        ACTIVE_MOVE_X.load(Ordering::Acquire),
        ACTIVE_MOVE_Y.load(Ordering::Acquire),
    ));

    ControlDiagnostics {
        selected_athlete: selected_athlete(),
        move_target: move_target(),
        attack_target,
        returning: active_kind == COMMAND_RETURN,
        attack_moving: active_kind == COMMAND_ATTACK_MOVE,
        attack_move_destination,
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
