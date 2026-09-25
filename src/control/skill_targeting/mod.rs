//! Cooldown gate around the validated generic skill-targeting implementation.
//!
//! The legacy targeting resolver remains byte-for-byte intact in `legacy.rs`. This wrapper adds one
//! pre-validation rule for manual control: if Q/W/R is currently on cooldown, pressing that skill is
//! a no-op. Do not arm targeting, draw a ray, queue a hostile-target chase, or queue a delayed cast.
//! The later cooldown-UI pass can add red feedback without changing this gameplay rule.

use std::sync::atomic::{AtomicU64, Ordering};

use mod_api_stable::{InputV1, StableAiContext};

// `legacy.rs` used to live directly at `control::skill_targeting`, so these aliases preserve its
// existing `super::entity_picker` and `super::set_active_attack` references after moving it one module
// deeper. No targeting/chase behavior is otherwise changed here.
use super::{entity_picker, set_active_attack};

mod legacy;

pub use legacy::{SkillPreviewMode, SkillSlot, SkillTargetingSnapshot};

const COOLDOWN_UNKNOWN: u64 = u64::MAX;

static Q_COOLDOWN: AtomicU64 = AtomicU64::new(COOLDOWN_UNKNOWN);
static W_COOLDOWN: AtomicU64 = AtomicU64::new(COOLDOWN_UNKNOWN);
static R_COOLDOWN: AtomicU64 = AtomicU64::new(COOLDOWN_UNKNOWN);

fn cooldown_atomic(slot: SkillSlot) -> &'static AtomicU64 {
    match slot {
        SkillSlot::Q => &Q_COOLDOWN,
        SkillSlot::W => &W_COOLDOWN,
        SkillSlot::R => &R_COOLDOWN,
    }
}

fn clear_cooldown_cache() {
    Q_COOLDOWN.store(COOLDOWN_UNKNOWN, Ordering::Release);
    W_COOLDOWN.store(COOLDOWN_UNKNOWN, Ordering::Release);
    R_COOLDOWN.store(COOLDOWN_UNKNOWN, Ordering::Release);
}

fn refresh_cooldown_cache(ctx: &mut StableAiContext<'_>) {
    let player_id = ctx.player_id();
    let values = {
        let Some(sim) = ctx.sim() else {
            return;
        };
        let Some(player) = sim.get_player(player_id) else {
            return;
        };
        player.cooldowns()
    };

    let Some((_attack, q, w, r)) = values else {
        return;
    };

    Q_COOLDOWN.store(q as u64, Ordering::Release);
    W_COOLDOWN.store(w as u64, Ordering::Release);
    R_COOLDOWN.store(r as u64, Ordering::Release);
}

pub fn cooldown_remaining(slot: SkillSlot) -> Option<usize> {
    let value = cooldown_atomic(slot).load(Ordering::Acquire);
    (value != COOLDOWN_UNKNOWN).then_some(value as usize)
}

fn known_on_cooldown(slot: SkillSlot) -> bool {
    matches!(cooldown_remaining(slot), Some(remaining) if remaining > 0)
}

pub fn reset() {
    legacy::reset();
    clear_cooldown_cache();
}

pub fn on_selection_changed() {
    legacy::on_selection_changed();
    clear_cooldown_cache();
}

pub fn arm(slot: SkillSlot) {
    // This check happens on the render/input side using the latest simulation-published cooldown.
    // A cooldown key press therefore leaves the player's existing move/attack/return order alone and
    // does not enter the targeting state that drives the helper ray.
    if known_on_cooldown(slot) {
        return;
    }
    legacy::arm(slot);
}

pub fn cancel() {
    legacy::cancel();
}

pub fn is_active() -> bool {
    legacy::is_active()
}

pub fn publish_cursor(x: u64, y: u64, sim_units_per_px: u64) {
    legacy::publish_cursor(x, y, sim_units_per_px);
}

pub fn clear_cursor() {
    legacy::clear_cursor();
}

pub fn confirm(x: u64, y: u64, sim_units_per_px: u64) {
    legacy::confirm(x, y, sim_units_per_px);
}

pub fn snapshot() -> SkillTargetingSnapshot {
    legacy::snapshot()
}

pub fn clamp_to_range(
    from: (u64, u64),
    to: (u64, u64),
    range: u64,
) -> (u64, u64) {
    legacy::clamp_to_range(from, to, range)
}

/// Refresh cooldown state on every selected-player simulation tick, even when no skill is armed.
/// The cached value lets the next physical Q/W/R press be rejected before the render-side targeting
/// UI appears. This authoritative second gate also catches the tiny race where a key press lands
/// between the skill becoming unavailable and the next cache publication.
pub fn manual_skill_input(
    ctx: &mut StableAiContext<'_>,
    self_position: Option<(u64, u64)>,
) -> Option<InputV1> {
    refresh_cooldown_cache(ctx);

    if let Some(slot) = legacy::armed() {
        if known_on_cooldown(slot) {
            legacy::cancel();
            return None;
        }
    }

    legacy::manual_skill_input(ctx, self_position)
}
