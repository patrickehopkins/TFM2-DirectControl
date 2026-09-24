# Future keybinding and playback-safety design

> **Status: post-release proposal, not current implementation instructions.** The validated behavior is described in `README.md` and `docs/core-control-contract.md`. This document records ideas for configurable Direct Control bindings and safe future speed features. Do not re-enable native replay seeking on `End` or implement the discarded variable-speed experiment from this document.

## Current validated v0.6.1 behavior

- `Ctrl+Home` starts after READY; `F1-F10` select the ten visible cards by stable athlete identity, not hard-coded team/role IDs.
- RMB performs contextual battlefield/minimap movement and attacks (including held RMB). `A` then LMB is attack-move; `H` Hold; `B` Return; Q/W/R cast skills; LMB confirms and RMB/Esc cancels armed targeting.
- `End` temporarily yields the selected champion to AI; MMB drag and wheel zoom still work. Crucially, paced-match ownership **continues**, so remappable native seek/highlight actions stay disabled.
- Confirmed `Ctrl+End` globally and irreversibly releases manual control and live pacing for the current match. Only then are native replay actions restored.
- An exact v0.6.1 native semantic-binding hook provides binding-independent replay-action suppression. It must be reverified, not assumed to work on another executable; see `docs/replay-native-action-analysis.md`.
- Ordinary synchronized pause/resume is supported. Harbinger uses a validated 1x / approximately 60 Hz live pacer; configurable Harbinger speed and death fast-forward are **not shipped**.

## Proposed first-class binding support (not implemented)

Inventory every player-facing action: readiness/start, ten visible-card selectors, contextual battlefield/minimap RMB, attack-move, Hold, Return, skill slots, targeting confirm/cancel, temporary yield, global release, camera pan/zoom, and any new actions added later.

A dedicated Direct Control category or context in the native Shortcuts Settings UI would be preferable if a robust SDK/native integration surface becomes available. Shared shortcuts with explicit conflict management are a fallback. Avoid globally overwriting the player's ordinary game bindings. The release's current fixed bindings remain documented in `README.md`; do not promise that configurable bindings already exist.

F1-F10 should continue to map to visible match cards rather than assuming player-team roles or internal simulation IDs. The validated mapping is detailed in `docs/core-control-contract.md`.

## Non-negotiable playback ownership

Native Back/Forward 10 Seconds, Previous/Next Highlight, and Highlight playback must remain semantically suppressed whenever Harbinger owns the live paced match, including after temporary `End` yield. A new shortcut UI must preserve that invariant for **remapped** keys. Native timeline pause must not decouple presentation from live simulation; continue using the existing synchronized pause/menu behavior.

Never implement a future spectator/control mode split by restoring replay-seeking shortcuts merely because the user has yielded the champion. The current suppression gate is in `src/replay_action_gate.rs`; accepted native UI paths and retest instructions are in `docs/replay-native-action-analysis.md`.

## Deferred: speed variation and a desync watchdog

The attempted synchronized-speed/death-fast-forward system did not reliably keep presentation coupled to the live simulation and was shelved before the first public release. Do **not** treat proposed 0.5x/1.5x/2x/3x rates or a death-speed shortcut as current features.

If that work resumes, use Candidate A as the authoritative live clock; derive exact presentation state from a verified native surface; prevent every seek/highlight path while the match is owned; and correct proven divergence by snapping presentation to live rather than accelerating playback to catch up. Change presentation and simulation speed together only after physical validation. If multiplayer work proceeds, **all Harbinger speed-affecting features must remain disabled in multiplayer and multiplayer must remain locked at 1x**. The mod does not currently claim multiplayer compatibility.

A snap-to-live watchdog is a future hardening idea, not a currently implemented fail-safe. It must respect the same ownership rule after `End`. The rejected experiments and investigation branches are recorded in `docs/deferred-investigations.md`.

## Related safety testing

Retest normal pause/resume, the `View Match Results Immediately` path when relevant, rebound native replay shortcuts, foreground-focus safety, and next-match gate reinstallation before claiming a changed input/playback implementation is safe. See `docs/replay-native-action-analysis.md` for the tested v0.6.1 regression sequence.
