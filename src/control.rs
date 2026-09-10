use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use mod_api_stable::{
    InputV1, SimOriginKindV1, StableAiContext, StableAiInit, StablePlayerAi,
};

const NO_PLAYER: usize = usize::MAX;

static SELECTED_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static MOVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static MOVE_X: AtomicU64 = AtomicU64::new(0);
static MOVE_Y: AtomicU64 = AtomicU64::new(0);
static MOVE_VERSION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
pub struct DirectControlAi;

pub fn reset() {
    SELECTED_PLAYER.store(NO_PLAYER, Ordering::Release);
    clear_move_target();
}

pub fn selected_player() -> Option<usize> {
    match SELECTED_PLAYER.load(Ordering::Acquire) {
        NO_PLAYER => None,
        player_id => Some(player_id),
    }
}

pub fn select_player(player_id: usize) {
    // Publish a brief unselected state so a newly selected player can never inherit the
    // previous player's last destination during a cross-thread tick boundary.
    SELECTED_PLAYER.store(NO_PLAYER, Ordering::Release);
    clear_move_target();
    SELECTED_PLAYER.store(player_id, Ordering::Release);
}

pub fn publish_move_target(x: u64, y: u64) {
    // Tiny seqlock: client/render code writes the pair, AI callbacks read it. Direct control
    // intentionally crosses the stable API's deterministic-simulation boundary, so this is
    // single-player only; the seqlock is for thread coherence, not replay determinism.
    MOVE_VERSION.fetch_add(1, Ordering::AcqRel); // odd = write in progress
    MOVE_X.store(x, Ordering::Relaxed);
    MOVE_Y.store(y, Ordering::Relaxed);
    MOVE_ACTIVE.store(true, Ordering::Relaxed);
    MOVE_VERSION.fetch_add(1, Ordering::Release); // even = stable snapshot
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

impl StablePlayerAi for DirectControlAi {
    fn clone_box(&self) -> Box<dyn StablePlayerAi> {
        Box::new(self.clone())
    }

    fn id(&self) -> String {
        "tfm2_direct_control.manual_input".to_owned()
    }

    fn priority(&self) -> i32 {
        10_000
    }

    fn matches(&self, _init: &StableAiInit) -> bool {
        // Attach to every player so F1-F10 can select any of the ten visible match slots.
        // When a player is not selected, think() returns the engine's base input unchanged.
        true
    }

    fn think(
        &mut self,
        ctx: &mut StableAiContext<'_>,
        base_input: Option<InputV1>,
    ) -> Option<InputV1> {
        if selected_player() != Some(ctx.player_id()) {
            return base_input;
        }

        // StablePlayerAi may also run in server pre-sims, replays, tools, or other invisible
        // simulations. Never let client mouse state leak into those simulations.
        let Some(sim) = ctx.sim() else {
            return base_input;
        };
        let Some(origin) = sim.sim_origin() else {
            return base_input;
        };
        if origin.kind != SimOriginKindV1::ClientMatchView.code() {
            return base_input;
        }

        if let Some((x, y)) = move_target() {
            return Some(InputV1::move_to(x, y));
        }

        // Selection itself should hand control to the user immediately, not let the selected
        // champion keep following vanilla AI until the first click. A move-to-self input is
        // the least invasive idle/stop command available through the stable API.
        let Some(player) = sim.get_player(ctx.player_id()) else {
            return base_input;
        };
        let Some(champion) = player.champion() else {
            return base_input;
        };
        let (x, y) = champion.pos();

        Some(InputV1::move_to(x, y))
    }
}
