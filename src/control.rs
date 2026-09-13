// Temporary wrapper around the previously validated control implementation.
//
// Keeping the old body byte-for-byte in control_base.rs makes this minimap correction narrowly
// scoped. Once minimap behavior is validated we can fold this helper back into control.rs normally.
include!("control_base.rs");

/// Publish a minimap RMB as an explicit ground MoveTo rather than an unresolved contextual RMB.
///
/// `src/lib.rs` still calls `publish_move_target()` immediately after `minimap::cursor_to_sim()`
/// returns. Until that call site is cleaned up, pre-mark the version that the legacy publisher is
/// about to create as already resolved. That keeps the contextual resolver from reinterpreting this
/// minimap click as Attack/Move and lets this direct move remain authoritative.
pub fn publish_minimap_move_target(x: u64, y: u64) {
    let x = x.min(DEFAULT_MAP_MAX_SIM);
    let y = y.min(DEFAULT_MAP_MAX_SIM);

    ATTACK_MOVE_ARMED.store(false, Ordering::Release);
    skill_targeting::cancel();
    clear_rmb_request();
    set_active_move(x, y);

    // publish_move_target() advances RMB_VERSION by exactly two (odd write marker, then stable even
    // version). Mark that imminent version resolved so resolve_latest_rmb() will ignore the legacy
    // request emitted by lib.rs on this same physical click.
    let next_legacy_version = RMB_VERSION.load(Ordering::Acquire).wrapping_add(2);
    RESOLVED_RMB_VERSION.store(next_legacy_version, Ordering::Release);
}
