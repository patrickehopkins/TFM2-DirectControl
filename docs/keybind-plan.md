# Direct Control keybind plan

## Goal

Before the first public release, Direct Control needs a first-class configurable control scheme rather than relying on hard-coded development bindings that collide with Teamfight Manager 2's spectator/playback shortcuts.

The preferred design is a **separate Direct Control shortcut mode/category** that becomes active only while a champion is under manual control. When Direct Control is inactive, the game's normal spectator shortcuts retain their native meanings. Selecting a champion for manual control enters the Direct Control scheme; `End` relinquishes the champion and returns to the native spectator scheme. `Ctrl+End` remains the stronger/global release.

This is preferred over simply mixing Direct Control actions into the existing spectator shortcut namespace. A shared namespace is an acceptable fallback if the game UI makes a separate mode impractical, but it creates avoidable conflicts. Direct Control must not simply overwrite native bindings globally.

## Current Direct Control action inventory

The configurable shortcut work should catalogue every player-facing action injected by the mod, including actions that use mouse buttons or chords:

- **Select player-team role 1-5** — currently F1-F5; exact native role ordering must be verified before the F-key mapping hardening pass.
- **Select opponent-team role 1-5** — currently F6-F10; same verified role order as the player team.
- **Contextual Move / Attack** — currently RMB on battlefield; ground = MoveTo, hostile entity = exact-target Attack.
- **Minimap Move** — currently RMB on minimap; should remain move-only once the minimap path is finalized.
- **Attack-Move** — currently `A` arms, LMB confirms destination.
- **Hold / Stop** — currently `H`.
- **Return Home** — currently `B`.
- **Skill Q** — currently `Q`.
- **Skill W** — currently `W`.
- **Skill R** — currently `R`.
- **Confirm armed action** — currently LMB for attack-move / skill confirmation.
- **Cancel armed action** — currently RMB or `Esc` as context permits.
- **Temporary release to AI / spectator** — currently `End`.
- **Start live paced simulation** — currently `Ctrl+Home`.
- **Emergency/global release** — currently `Ctrl+End`.
- **Camera movement bindings** — once the pre-release camera-control item is implemented, any keyboard camera-pan actions added there must be included in the same configurable Direct Control category.

Automatic behaviors such as team fog-of-war switching are not bindings and should not appear as shortcut entries unless a later explicit toggle is added.

## Preferred mode behavior

Direct Control shortcut mode should be active only while manual champion ownership is active. This gives overlapping keys safe context-sensitive meanings:

- spectator mode: native Teamfight Manager 2 shortcut behavior;
- Direct Control mode: Direct Control combat/movement bindings take priority;
- `End`: leave Direct Control mode and restore native spectator shortcuts immediately;
- `Ctrl+End`: global/emergency release remains available regardless of the active scheme.

This allows optimal MOBA-like defaults such as `A`, `Q`, and `R` without requiring the player to permanently sacrifice the game's spectator controls.

## Playback-desync safety gate

While Direct Control mode is active, native playback/navigation commands that can move the presentation away from the live paced simulation must be suppressed even if the player has rebound them to non-conflicting keys. This is a semantic gate, not merely a default-key conflict fix.

At minimum, gate the native commands that alter playback position or playback rate/state:

- Back 10 Seconds;
- Forward 10 Seconds;
- Previous Highlight;
- Next Highlight;
- Pause Match;
- playback speed controls (0.5x / 1x / 1.5x / 2x / 3x);
- Highlight Mode or any equivalent playback mode that can jump/decouple presentation from the live point.

Reason: rewinding while Direct Control is live leaves the presentation behind the still-advancing simulation; the only practical recovery is fast-forwarding until the display catches up. Direct Control should prevent entering that desynchronized state in the first place.

Native spectator-only functions that do not alter playback position/rate can remain available unless they conflict with an active Direct Control binding or another pre-release system. F1-F10 native Follow Own/Enemy actions are naturally replaced by manual champion selection while Direct Control mode is active and return to normal when control is released.

## Shortcut settings UI

Preferred UI:

- add a **Direct Control** category to the game's Shortcuts Settings menu;
- show the Direct Control actions there with configurable bindings and the current bindings as defaults;
- make it clear that these bindings are active only during manual champion control where applicable;
- preserve the user's native spectator bindings unchanged.

Fallback UI if a separate mode/category proves disproportionately invasive:

- place Direct Control entries in the normal Shortcuts Settings system and use conflict detection/clear labeling;
- still apply the playback-desync safety gate while Direct Control is active.

Explicitly rejected approach: globally override the game's native shortcuts with Direct Control defaults regardless of mode.
