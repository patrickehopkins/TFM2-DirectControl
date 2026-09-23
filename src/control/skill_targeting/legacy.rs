//! Generic skill-targeting state shared between the render thread and paced player-AI callback.
//!
//! The render thread publishes only physical-control intent: logical skill slot, cursor position,
//! and LMB confirmation. The paced simulation thread decides which InputTarget shape the selected
//! champion actually accepts through StableAiContext::is_valid_input.
//!
//! Preview mode is deliberately only a hint. Vanilla action metadata is not exposed through the
//! stable runtime AI context, and situational target requirements can make a legal Target skill look
//! like another shape while no valid target exists. Every LMB confirmation therefore probes the
//! legal forms again instead of trusting a cached preview guess.
//!
//! Hostile single-target skills have one extra direct-control behavior: if the click identifies a
//! legal/credible hostile Target skill but that exact cast is currently out of range, the simulation
//! retains the entity id and moves toward it until the cast becomes legal. After the cast is emitted,
//! the same entity becomes the normal exact-target attack/chase order in the parent control module.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use mod_api_stable::{InputKindV1, InputTargetKindV1, InputTargetV1, InputV1, StableAiContext};

use super::entity_picker::{pick_entity, EntityPick, TeamRelation};

const SLOT_NONE: u8 = 0;
const SLOT_Q: u8 = 1;
const SLOT_W: u8 = 2;
const SLOT_R: u8 = 3;
const NO_TARGET: usize = usize::MAX;

const MODE_UNKNOWN: u8 = 0;
const MODE_TARGET: u8 = 1;
const MODE_POSITION: u8 = 2;
const MODE_DIRECTION: u8 = 3;
const MODE_NONE: u8 = 4;

const RANGE_UNCHECKED: u8 = 0;
const RANGE_KNOWN: u8 = 1;
const RANGE_UNAVAILABLE: u8 = 2;

const DEFAULT_MAP_MAX_SIM: f64 = 960_000.0;
const RANGE_SEARCH_STEPS: usize = 18;

static ARMED_SLOT: AtomicU8 = AtomicU8::new(SLOT_NONE);

// Out-of-range hostile Target casts leave the ordinary cursor-confirm state and enter this retained
// simulation-side chase. Only one exact entity is retained, and loss of legal vision drops it.
static PENDING_CHASE_SLOT: AtomicU8 = AtomicU8::new(SLOT_NONE);
static PENDING_CHASE_TARGET: AtomicUsize = AtomicUsize::new(NO_TARGET);

static CURSOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static CURSOR_X: AtomicU64 = AtomicU64::new(0);
static CURSOR_Y: AtomicU64 = AtomicU64::new(0);
static CURSOR_VERSION: AtomicU64 = AtomicU64::new(0);

static CONFIRM_ACTIVE: AtomicBool = AtomicBool::new(false);
static CONFIRM_X: AtomicU64 = AtomicU64::new(0);
static CONFIRM_Y: AtomicU64 = AtomicU64::new(0);
static CONFIRM_VERSION: AtomicU64 = AtomicU64::new(0);
static RESOLVED_CONFIRM_VERSION: AtomicU64 = AtomicU64::new(0);

static PREVIEW_MODE: AtomicU8 = AtomicU8::new(MODE_UNKNOWN);
static PREVIEW_RANGE_STATE: AtomicU8 = AtomicU8::new(RANGE_UNCHECKED);
static PREVIEW_RANGE_SIM: AtomicU64 = AtomicU64::new(0);
static SELF_ACTIVE: AtomicBool = AtomicBool::new(false);
static SELF_X: AtomicU64 = AtomicU64::new(0);
static SELF_Y: AtomicU64 = AtomicU64::new(0);

static ARM_COUNT: AtomicU64 = AtomicU64::new(0);
static CANCEL_COUNT: AtomicU64 = AtomicU64::new(0);
static CONFIRM_COUNT: AtomicU64 = AtomicU64::new(0);
static CAST_COUNT: AtomicU64 = AtomicU64::new(0);
static REJECT_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSlot {
    Q,
    W,
    R,
}

impl SkillSlot {
    pub fn label(self) -> &'static str {
        match self {
            Self::Q => "Q",
            Self::W => "W",
            Self::R => "R",
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Q => SLOT_Q,
            Self::W => SLOT_W,
            Self::R => SLOT_R,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            SLOT_Q => Some(Self::Q),
            SLOT_W => Some(Self::W),
            SLOT_R => Some(Self::R),
            _ => None,
        }
    }

    fn input_kind(self) -> InputKindV1 {
        match self {
            Self::Q => InputKindV1::Skill,
            Self::W => InputKindV1::Skill2,
            Self::R => InputKindV1::Ult,
        }
    }

    fn minimum_level(self) -> usize {
        match self {
            Self::Q => 1,
            Self::W => 3,
            Self::R => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillPreviewMode {
    Unknown,
    Target,
    Position,
    Direction,
    None,
}

impl SkillPreviewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "?",
            Self::Target => "TARGET",
            Self::Position => "POSITION",
            Self::Direction => "DIRECTION",
            Self::None => "SELF/NONE",
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Unknown => MODE_UNKNOWN,
            Self::Target => MODE_TARGET,
            Self::Position => MODE_POSITION,
            Self::Direction => MODE_DIRECTION,
            Self::None => MODE_NONE,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            MODE_TARGET => Self::Target,
            MODE_POSITION => Self::Position,
            MODE_DIRECTION => Self::Direction,
            MODE_NONE => Self::None,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SkillTargetingSnapshot {
    pub armed: Option<SkillSlot>,
    pub mode: SkillPreviewMode,
    pub cursor: Option<(u64, u64)>,
    pub self_position: Option<(u64, u64)>,
    /// Present only when runtime validation found a finite boundary. Map-edge acceptance remains
    /// unknown rather than being displayed as a fake map-sized range.
    pub range_sim: Option<u64>,
    pub arm_count: u64,
    pub cancel_count: u64,
    pub confirm_count: u64,
    pub cast_count: u64,
    pub reject_count: u64,
}

#[derive(Debug)]
enum ConfirmResolution {
    Cast {
        input: InputV1,
        hostile_follow_up: Option<usize>,
    },
    ChaseHostile(usize),
    Reject,
}

fn clear_preview() {
    PREVIEW_MODE.store(MODE_UNKNOWN, Ordering::Release);
    PREVIEW_RANGE_STATE.store(RANGE_UNCHECKED, Ordering::Release);
    PREVIEW_RANGE_SIM.store(0, Ordering::Relaxed);
    SELF_ACTIVE.store(false, Ordering::Release);
    SELF_X.store(0, Ordering::Relaxed);
    SELF_Y.store(0, Ordering::Relaxed);
}

fn clear_confirm() {
    CONFIRM_VERSION.fetch_add(1, Ordering::AcqRel);
    CONFIRM_ACTIVE.store(false, Ordering::Relaxed);
    CONFIRM_X.store(0, Ordering::Relaxed);
    CONFIRM_Y.store(0, Ordering::Relaxed);
    let stable = CONFIRM_VERSION.fetch_add(1, Ordering::Release) + 1;
    RESOLVED_CONFIRM_VERSION.store(stable, Ordering::Release);
}

fn clear_targeting_state() {
    ARMED_SLOT.store(SLOT_NONE, Ordering::Release);
    CURSOR_ACTIVE.store(false, Ordering::Release);
    CURSOR_X.store(0, Ordering::Relaxed);
    CURSOR_Y.store(0, Ordering::Relaxed);
    clear_confirm();
    clear_preview();
}

fn clear_pending_chase() {
    PENDING_CHASE_TARGET.store(NO_TARGET, Ordering::Relaxed);
    PENDING_CHASE_SLOT.store(SLOT_NONE, Ordering::Release);
}

fn pending_chase() -> Option<(SkillSlot, usize)> {
    let slot = SkillSlot::from_code(PENDING_CHASE_SLOT.load(Ordering::Acquire))?;
    let target = PENDING_CHASE_TARGET.load(Ordering::Acquire);
    (target != NO_TARGET).then_some((slot, target))
}

fn start_pending_chase(slot: SkillSlot, target_id: usize) {
    // Confirmation is complete; hide the free cursor and retain only authoritative simulation-side
    // identity. PREVIEW_MODE remains Target for the overlay while the chase is active.
    clear_targeting_state();
    PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
    PENDING_CHASE_TARGET.store(target_id, Ordering::Relaxed);
    PENDING_CHASE_SLOT.store(slot.code(), Ordering::Release);
}

pub fn reset() {
    clear_targeting_state();
    clear_pending_chase();
    CURSOR_VERSION.store(0, Ordering::Release);
    CONFIRM_VERSION.store(0, Ordering::Release);
    RESOLVED_CONFIRM_VERSION.store(0, Ordering::Release);
    ARM_COUNT.store(0, Ordering::Release);
    CANCEL_COUNT.store(0, Ordering::Release);
    CONFIRM_COUNT.store(0, Ordering::Release);
    CAST_COUNT.store(0, Ordering::Release);
    REJECT_COUNT.store(0, Ordering::Release);
}

pub fn on_selection_changed() {
    clear_targeting_state();
    clear_pending_chase();
}

pub fn arm(slot: SkillSlot) {
    clear_pending_chase();
    ARMED_SLOT.store(slot.code(), Ordering::Release);
    clear_confirm();
    clear_preview();
    ARM_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn cancel() {
    if armed().is_some() || pending_chase().is_some() {
        CANCEL_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    clear_targeting_state();
    clear_pending_chase();
}

pub fn armed() -> Option<SkillSlot> {
    SkillSlot::from_code(ARMED_SLOT.load(Ordering::Acquire))
}

fn active_slot() -> Option<SkillSlot> {
    armed().or_else(|| pending_chase().map(|(slot, _)| slot))
}

pub fn is_active() -> bool {
    active_slot().is_some()
}

pub fn publish_cursor(x: u64, y: u64) {
    CURSOR_VERSION.fetch_add(1, Ordering::AcqRel);
    CURSOR_X.store(x, Ordering::Relaxed);
    CURSOR_Y.store(y, Ordering::Relaxed);
    CURSOR_ACTIVE.store(true, Ordering::Relaxed);
    CURSOR_VERSION.fetch_add(1, Ordering::Release);
}

pub fn clear_cursor() {
    CURSOR_VERSION.fetch_add(1, Ordering::AcqRel);
    CURSOR_ACTIVE.store(false, Ordering::Relaxed);
    CURSOR_X.store(0, Ordering::Relaxed);
    CURSOR_Y.store(0, Ordering::Relaxed);
    CURSOR_VERSION.fetch_add(1, Ordering::Release);
}

pub fn confirm(x: u64, y: u64) {
    CONFIRM_VERSION.fetch_add(1, Ordering::AcqRel);
    CONFIRM_X.store(x, Ordering::Relaxed);
    CONFIRM_Y.store(y, Ordering::Relaxed);
    CONFIRM_ACTIVE.store(true, Ordering::Relaxed);
    CONFIRM_VERSION.fetch_add(1, Ordering::Release);
    CONFIRM_COUNT.fetch_add(1, Ordering::Relaxed);
}

fn cursor_snapshot() -> Option<(u64, u64)> {
    for _ in 0..4 {
        let before = CURSOR_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }
        let active = CURSOR_ACTIVE.load(Ordering::Relaxed);
        let x = CURSOR_X.load(Ordering::Relaxed);
        let y = CURSOR_Y.load(Ordering::Relaxed);
        let after = CURSOR_VERSION.load(Ordering::Acquire);
        if before == after && after & 1 == 0 {
            return active.then_some((x, y));
        }
    }
    None
}

fn confirm_snapshot() -> Option<(u64, u64, u64)> {
    for _ in 0..4 {
        let before = CONFIRM_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }
        let active = CONFIRM_ACTIVE.load(Ordering::Relaxed);
        let x = CONFIRM_X.load(Ordering::Relaxed);
        let y = CONFIRM_Y.load(Ordering::Relaxed);
        let after = CONFIRM_VERSION.load(Ordering::Acquire);
        if before == after && after & 1 == 0 {
            return active.then_some((x, y, after));
        }
    }
    None
}

fn action(slot: SkillSlot, target: InputTargetV1) -> InputV1 {
    InputV1::action(slot.input_kind(), target)
}

fn target_none() -> InputTargetV1 {
    InputTargetV1 {
        kind: InputTargetKindV1::None.code(),
        ..Default::default()
    }
}

fn target_pos(x: u64, y: u64) -> InputTargetV1 {
    InputTargetV1 {
        kind: InputTargetKindV1::Pos.code(),
        x,
        y,
        ..Default::default()
    }
}

fn target_entity(target_id: usize) -> InputTargetV1 {
    InputTargetV1 {
        kind: InputTargetKindV1::Target.code(),
        target_id,
        ..Default::default()
    }
}

fn target_dir(from: (u64, u64), to: (u64, u64)) -> InputTargetV1 {
    let mut dx = to.0 as i128 - from.0 as i128;
    let dy = to.1 as i128 - from.1 as i128;
    if dx == 0 && dy == 0 {
        dx = 1;
    }
    let clamp = |v: i128| v.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    InputTargetV1 {
        kind: InputTargetKindV1::Dir.code(),
        dir_x: clamp(dx),
        dir_y: clamp(dy),
        ..Default::default()
    }
}

fn slot_unlocked(ctx: &mut StableAiContext<'_>, slot: SkillSlot) -> Option<bool> {
    let player_id = ctx.player_id();
    let sim = ctx.sim()?;
    let player = sim.get_player(player_id)?;
    Some(player.level() >= slot.minimum_level())
}

fn slot_cooldown(ctx: &mut StableAiContext<'_>, slot: SkillSlot) -> Option<usize> {
    let player_id = ctx.player_id();
    let sim = ctx.sim()?;
    let player = sim.get_player(player_id)?;
    let cooldowns = player.cooldowns()?;
    Some(match slot {
        SkillSlot::Q => cooldowns.1,
        SkillSlot::W => cooldowns.2,
        SkillSlot::R => cooldowns.3,
    })
}

fn clicked_entity(ctx: &mut StableAiContext<'_>, click: (u64, u64)) -> Option<EntityPick> {
    let team = ctx.team();
    let sim = ctx.sim()?;
    pick_entity(
        &sim,
        team,
        TeamRelation::Any,
        true,
        click.0,
        click.1,
        0,
    )
}

fn own_entity_id(ctx: &mut StableAiContext<'_>) -> Option<usize> {
    let player_id = ctx.player_id();
    let sim = ctx.sim()?;
    let player = sim.get_player(player_id)?;
    let champion = player.champion()?;
    Some(champion.id())
}

fn visible_target_ids(ctx: &mut StableAiContext<'_>) -> Vec<(usize, usize)> {
    let team = ctx.team();
    let Some(sim) = ctx.sim() else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for index in 0..sim.entity_count() {
        let Some(entity) = sim.entity_at(index) else {
            continue;
        };
        if entity.is_alive() && entity.is_targetable() && sim.is_visible(team, entity.id()) {
            ids.push((entity.id(), entity.team()));
        }
    }
    ids
}

/// Infer only a presentation hint. This is deliberately re-evaluated while armed and is never used
/// as the sole authority for LMB execution.
fn infer_mode(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
    cursor: (u64, u64),
) -> SkillPreviewMode {
    if ctx.is_valid_input(&action(slot, target_none())) {
        return SkillPreviewMode::None;
    }

    // Some vanilla self buffs are represented as a Targeting action with AllyOnlySelf rather than
    // CastingType::None. Probe our own champion explicitly so those do not masquerade as Direction.
    if let Some(self_id) = own_entity_id(ctx) {
        if ctx.is_valid_input(&action(slot, target_entity(self_id))) {
            return SkillPreviewMode::Target;
        }
    }

    if let Some(picked) = clicked_entity(ctx, cursor) {
        if ctx.is_valid_input(&action(slot, target_entity(picked.id))) {
            return SkillPreviewMode::Target;
        }
    }
    for (id, _) in visible_target_ids(ctx).into_iter().take(64) {
        if ctx.is_valid_input(&action(slot, target_entity(id))) {
            return SkillPreviewMode::Target;
        }
    }

    if ctx.is_valid_input(&action(slot, target_dir(self_position, cursor))) {
        return SkillPreviewMode::Direction;
    }

    if ctx.is_valid_input(&action(slot, target_pos(self_position.0, self_position.1))) {
        return SkillPreviewMode::Position;
    }

    SkillPreviewMode::Unknown
}

fn point_at_distance(from: (u64, u64), toward: (f64, f64), distance: u64) -> (u64, u64) {
    let x = from.0 as f64 + toward.0 * distance as f64;
    let y = from.1 as f64 + toward.1 * distance as f64;
    (
        x.clamp(0.0, DEFAULT_MAP_MAX_SIM).round() as u64,
        y.clamp(0.0, DEFAULT_MAP_MAX_SIM).round() as u64,
    )
}

fn direction_toward_map_center(from: (u64, u64)) -> (f64, f64) {
    let center = DEFAULT_MAP_MAX_SIM * 0.5;
    let mut dx = center - from.0 as f64;
    let mut dy = center - from.1 as f64;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return (1.0, 0.0);
    }
    dx /= len;
    dy /= len;
    (dx, dy)
}

fn distance_to_map_edge(from: (u64, u64), dir: (f64, f64)) -> u64 {
    let x = from.0 as f64;
    let y = from.1 as f64;
    let tx = if dir.0 > 0.0 {
        (DEFAULT_MAP_MAX_SIM - x) / dir.0
    } else if dir.0 < 0.0 {
        (0.0 - x) / dir.0
    } else {
        f64::INFINITY
    };
    let ty = if dir.1 > 0.0 {
        (DEFAULT_MAP_MAX_SIM - y) / dir.1
    } else if dir.1 < 0.0 {
        (0.0 - y) / dir.1
    } else {
        f64::INFINITY
    };
    tx.min(ty).max(0.0).floor() as u64
}

/// Returns a finite validator-backed Position range only if the validator supplies an actual upper
/// boundary before the map edge. Accepting the edge means "not inferable", not "range = whole map".
fn infer_position_range(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
) -> Option<u64> {
    if !ctx.is_valid_input(&action(
        slot,
        target_pos(self_position.0, self_position.1),
    )) {
        return None;
    }

    let dir = direction_toward_map_center(self_position);
    let mut low = 0u64;
    let mut high = distance_to_map_edge(self_position, dir);
    if high == 0 {
        return None;
    }

    let far = point_at_distance(self_position, dir, high);
    if ctx.is_valid_input(&action(slot, target_pos(far.0, far.1))) {
        return None;
    }

    for _ in 0..RANGE_SEARCH_STEPS {
        if high <= low.saturating_add(1) {
            break;
        }
        let mid = low + (high - low) / 2;
        let point = point_at_distance(self_position, dir, mid);
        if ctx.is_valid_input(&action(slot, target_pos(point.0, point.1))) {
            low = mid;
        } else {
            high = mid;
        }
    }

    (low > 0).then_some(low)
}

pub fn clamp_to_range(from: (u64, u64), to: (u64, u64), range: u64) -> (u64, u64) {
    if range == 0 {
        return from;
    }
    let dx = to.0 as f64 - from.0 as f64;
    let dy = to.1 as f64 - from.1 as f64;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= range as f64 || distance < 1.0 {
        return to;
    }
    let scale = range as f64 / distance;
    (
        (from.0 as f64 + dx * scale).round().max(0.0) as u64,
        (from.1 as f64 + dy * scale).round().max(0.0) as u64,
    )
}

fn update_preview(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
) {
    SELF_X.store(self_position.0, Ordering::Relaxed);
    SELF_Y.store(self_position.1, Ordering::Relaxed);
    SELF_ACTIVE.store(true, Ordering::Release);

    let Some(cursor) = cursor_snapshot() else {
        return;
    };

    let inferred = infer_mode(ctx, slot, self_position, cursor);
    let previous = SkillPreviewMode::from_code(PREVIEW_MODE.load(Ordering::Acquire));
    let mode = if inferred != SkillPreviewMode::Unknown {
        if inferred != previous {
            PREVIEW_MODE.store(inferred.code(), Ordering::Release);
            PREVIEW_RANGE_STATE.store(RANGE_UNCHECKED, Ordering::Release);
            PREVIEW_RANGE_SIM.store(0, Ordering::Relaxed);
        }
        inferred
    } else {
        previous
    };

    if PREVIEW_RANGE_STATE.load(Ordering::Acquire) != RANGE_UNCHECKED {
        return;
    }

    if mode == SkillPreviewMode::Position {
        if let Some(range) = infer_position_range(ctx, slot, self_position) {
            PREVIEW_RANGE_SIM.store(range, Ordering::Relaxed);
            PREVIEW_RANGE_STATE.store(RANGE_KNOWN, Ordering::Release);
        } else {
            PREVIEW_RANGE_STATE.store(RANGE_UNAVAILABLE, Ordering::Release);
        }
    } else if mode != SkillPreviewMode::Unknown {
        PREVIEW_RANGE_STATE.store(RANGE_UNAVAILABLE, Ordering::Release);
    }
}

fn legal_target_relations(ctx: &mut StableAiContext<'_>, slot: SkillSlot) -> (bool, bool) {
    let controlled_team = ctx.team();
    let mut hostile = false;
    let mut friendly = false;
    for (id, team) in visible_target_ids(ctx).into_iter().take(64) {
        if !ctx.is_valid_input(&action(slot, target_entity(id))) {
            continue;
        }
        if team == controlled_team {
            friendly = true;
        } else {
            hostile = true;
        }
        if hostile && friendly {
            break;
        }
    }
    (hostile, friendly)
}

fn legal_target_exists(ctx: &mut StableAiContext<'_>, slot: SkillSlot) -> bool {
    let (hostile, friendly) = legal_target_relations(ctx, slot);
    hostile || friendly
}

fn immediate_self_cast(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
) -> Option<InputV1> {
    // CastingType::None is inherently cursorless. If the runtime accepts it now, key press is the
    // complete command and there is no reason to enter Direct Control's click-confirm flow.
    let none = action(slot, target_none());
    if ctx.is_valid_input(&none) {
        PREVIEW_MODE.store(MODE_NONE, Ordering::Release);
        return Some(none);
    }

    // Some vanilla self buffs are encoded as Targeting + AllyOnlySelf rather than CastingType::None.
    // StableAiContext does not expose the live base action definition, so classify conservatively
    // from validator evidence: self must be legal, every other currently visible entity target must
    // be illegal, and the Position/Direction forms must also reject. Ordinary entity-target skills
    // therefore retain their existing click-confirm path whenever another legal target exists.
    let self_id = own_entity_id(ctx)?;
    let self_targeted = action(slot, target_entity(self_id));
    if !ctx.is_valid_input(&self_targeted) {
        return None;
    }

    for (id, _) in visible_target_ids(ctx).into_iter().take(64) {
        if id != self_id && ctx.is_valid_input(&action(slot, target_entity(id))) {
            return None;
        }
    }

    if ctx.is_valid_input(&action(slot, target_dir(self_position, self_position)))
        || ctx.is_valid_input(&action(
            slot,
            target_pos(self_position.0, self_position.1),
        ))
    {
        return None;
    }

    PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
    Some(self_targeted)
}

/// Returns true only when an invalid hostile Target click has enough runtime evidence to mean
/// "correct target shape, currently out of range" rather than "this slot is Direction/Position/etc".
/// When another legal entity target exists, relation is authoritative. When every entity is out of
/// range, we fall back only if the skill is ready and all non-hostile-target shapes reject.
fn hostile_target_chase_plausible(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
    click: (u64, u64),
) -> bool {
    match SkillPreviewMode::from_code(PREVIEW_MODE.load(Ordering::Acquire)) {
        SkillPreviewMode::Direction | SkillPreviewMode::Position | SkillPreviewMode::None => {
            return false;
        }
        SkillPreviewMode::Unknown | SkillPreviewMode::Target => {}
    }

    let (hostile_legal, friendly_legal) = legal_target_relations(ctx, slot);
    if hostile_legal {
        return true;
    }
    if friendly_legal {
        // Runtime has demonstrated an ally/self Target action, but no hostile Target action.
        return false;
    }

    if slot_cooldown(ctx, slot) != Some(0) {
        return false;
    }

    if ctx.is_valid_input(&action(slot, target_none())) {
        return false;
    }
    if let Some(self_id) = own_entity_id(ctx) {
        if ctx.is_valid_input(&action(slot, target_entity(self_id))) {
            return false;
        }
    }
    if ctx.is_valid_input(&action(slot, target_dir(self_position, click))) {
        return false;
    }
    if ctx.is_valid_input(&action(
        slot,
        target_pos(self_position.0, self_position.1),
    )) {
        return false;
    }

    true
}

/// Resolve the actual click independently of the preview guess. This is the execution authority.
fn resolve_confirm(
    ctx: &mut StableAiContext<'_>,
    slot: SkillSlot,
    self_position: (u64, u64),
    click: (u64, u64),
) -> ConfirmResolution {
    let none = action(slot, target_none());
    if ctx.is_valid_input(&none) {
        PREVIEW_MODE.store(MODE_NONE, Ordering::Release);
        return ConfirmResolution::Cast {
            input: none,
            hostile_follow_up: None,
        };
    }

    let clicked = clicked_entity(ctx, click);
    if let Some(ref picked) = clicked {
        let targeted = action(slot, target_entity(picked.id));
        if ctx.is_valid_input(&targeted) {
            PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
            let hostile_follow_up = (picked.team != ctx.team()).then_some(picked.id);
            return ConfirmResolution::Cast {
                input: targeted,
                hostile_follow_up,
            };
        }

        if picked.team != ctx.team()
            && hostile_target_chase_plausible(ctx, slot, self_position, click)
        {
            PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
            return ConfirmResolution::ChaseHostile(picked.id);
        }
    } else if let Some(self_id) = own_entity_id(ctx) {
        // Blank-map LMB is allowed to confirm a self-only Targeting buff. We intentionally do not
        // auto-self-cast when the user actually clicked another entity: a failed ally/enemy click
        // should remain a failed target selection, not silently redirect onto the caster.
        let self_targeted = action(slot, target_entity(self_id));
        if ctx.is_valid_input(&self_targeted) {
            PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
            return ConfirmResolution::Cast {
                input: self_targeted,
                hostile_follow_up: None,
            };
        }
    }

    // If this action demonstrably accepts entity targets right now, a wrong entity click is a
    // rejection. Do not fall through to a permissive Direction/Position interpretation.
    if legal_target_exists(ctx, slot) {
        PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
        return ConfirmResolution::Reject;
    }

    let direction = action(slot, target_dir(self_position, click));
    if ctx.is_valid_input(&direction) {
        PREVIEW_MODE.store(MODE_DIRECTION, Ordering::Release);
        return ConfirmResolution::Cast {
            input: direction,
            hostile_follow_up: None,
        };
    }

    let range = match PREVIEW_RANGE_STATE.load(Ordering::Acquire) {
        RANGE_KNOWN => Some(PREVIEW_RANGE_SIM.load(Ordering::Acquire)),
        _ => None,
    };
    let point = range
        .map(|range| clamp_to_range(self_position, click, range))
        .unwrap_or(click);
    let position = action(slot, target_pos(point.0, point.1));
    if ctx.is_valid_input(&position) {
        PREVIEW_MODE.store(MODE_POSITION, Ordering::Release);
        return ConfirmResolution::Cast {
            input: position,
            hostile_follow_up: None,
        };
    }

    ConfirmResolution::Reject
}

fn pending_chase_input(
    ctx: &mut StableAiContext<'_>,
    self_position: (u64, u64),
    slot: SkillSlot,
    target_id: usize,
) -> Option<InputV1> {
    // Preserve the locked-slot safety invariant even though a pending chase can only be created after
    // the first unlocked confirmation. Future level/state changes should never bypass this guard.
    if slot_unlocked(ctx, slot) != Some(true) {
        REJECT_COUNT.fetch_add(1, Ordering::Relaxed);
        clear_pending_chase();
        clear_targeting_state();
        return None;
    }

    let controlled_team = ctx.team();
    let target_position = {
        let Some(sim) = ctx.sim() else {
            return Some(InputV1::move_to(self_position.0, self_position.1));
        };
        let Some(target) = sim.get_entity(target_id) else {
            REJECT_COUNT.fetch_add(1, Ordering::Relaxed);
            clear_pending_chase();
            clear_targeting_state();
            return None;
        };
        if !target.is_alive()
            || !target.is_targetable()
            || target.team() == controlled_team
            || !sim.is_visible(controlled_team, target_id)
        {
            REJECT_COUNT.fetch_add(1, Ordering::Relaxed);
            clear_pending_chase();
            clear_targeting_state();
            return None;
        }
        target.pos()
    };

    let cast = action(slot, target_entity(target_id));
    if ctx.is_valid_input(&cast) {
        CAST_COUNT.fetch_add(1, Ordering::Relaxed);
        clear_pending_chase();
        clear_targeting_state();
        // The skill wins this tick; on the next simulation update the normal exact-target command
        // takes over and inherits the validated attack/chase timing and vision-loss rules.
        super::set_active_attack(target_id);
        return Some(cast);
    }

    let chase = InputV1::move_to(target_position.0, target_position.1);
    if ctx.is_valid_input(&chase) {
        return Some(chase);
    }

    // If movement is temporarily uncancellable, hold rather than letting an unrelated older order
    // resume. TFM2's validator remains the authority for when chase movement becomes legal.
    Some(InputV1::move_to(self_position.0, self_position.1))
}

/// Called only for the already-selected athlete from the paced StablePlayerAi callback.
/// Returns Some for a successful one-tick skill cast or while an accepted hostile Target skill is
/// actively chasing its retained entity. Otherwise the persistent parent control order remains in force.
pub fn manual_skill_input(
    ctx: &mut StableAiContext<'_>,
    self_position: Option<(u64, u64)>,
) -> Option<InputV1> {
    let self_position = self_position?;

    if let Some((slot, target_id)) = pending_chase() {
        SELF_X.store(self_position.0, Ordering::Relaxed);
        SELF_Y.store(self_position.1, Ordering::Relaxed);
        SELF_ACTIVE.store(true, Ordering::Release);
        PREVIEW_MODE.store(MODE_TARGET, Ordering::Release);
        return pending_chase_input(ctx, self_position, slot, target_id);
    }

    let slot = armed()?;

    // Never ask the game's validator about a skill that does not exist for this champion level yet.
    // Physical testing found that probing a locked W/R can wedge the watched simulation worker rather
    // than simply return false. The game's skill progression is Q at level 1, W at level 3, R at
    // level 5, so reject locked slots before any is_valid_input call or preview/range probing.
    let unlocked = slot_unlocked(ctx, slot)?;
    if !unlocked {
        REJECT_COUNT.fetch_add(1, Ordering::Relaxed);
        clear_targeting_state();
        return None;
    }

    if let Some(input) = immediate_self_cast(ctx, slot, self_position) {
        CAST_COUNT.fetch_add(1, Ordering::Relaxed);
        clear_targeting_state();
        return Some(input);
    }

    update_preview(ctx, slot, self_position);

    let Some((x, y, version)) = confirm_snapshot() else {
        return None;
    };
    if RESOLVED_CONFIRM_VERSION.load(Ordering::Acquire) == version {
        return None;
    }

    RESOLVED_CONFIRM_VERSION.store(version, Ordering::Release);
    CONFIRM_ACTIVE.store(false, Ordering::Release);

    match resolve_confirm(ctx, slot, self_position, (x, y)) {
        ConfirmResolution::Cast {
            input,
            hostile_follow_up,
        } => {
            CAST_COUNT.fetch_add(1, Ordering::Relaxed);
            clear_targeting_state();
            if let Some(target_id) = hostile_follow_up {
                super::set_active_attack(target_id);
            }
            Some(input)
        }
        ConfirmResolution::ChaseHostile(target_id) => {
            start_pending_chase(slot, target_id);
            pending_chase_input(ctx, self_position, slot, target_id)
        }
        ConfirmResolution::Reject => {
            REJECT_COUNT.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

pub fn snapshot() -> SkillTargetingSnapshot {
    let cursor = cursor_snapshot();
    let self_position = if SELF_ACTIVE.load(Ordering::Acquire) {
        Some((
            SELF_X.load(Ordering::Relaxed),
            SELF_Y.load(Ordering::Relaxed),
        ))
    } else {
        None
    };
    let range = (PREVIEW_RANGE_STATE.load(Ordering::Acquire) == RANGE_KNOWN)
        .then(|| PREVIEW_RANGE_SIM.load(Ordering::Acquire));

    SkillTargetingSnapshot {
        armed: active_slot(),
        mode: if pending_chase().is_some() {
            SkillPreviewMode::Target
        } else {
            SkillPreviewMode::from_code(PREVIEW_MODE.load(Ordering::Acquire))
        },
        cursor,
        self_position,
        range_sim: range,
        arm_count: ARM_COUNT.load(Ordering::Acquire),
        cancel_count: CANCEL_COUNT.load(Ordering::Acquire),
        confirm_count: CONFIRM_COUNT.load(Ordering::Acquire),
        cast_count: CAST_COUNT.load(Ordering::Acquire),
        reject_count: REJECT_COUNT.load(Ordering::Acquire),
    }
}
