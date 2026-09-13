# Direct Control release scope

This file records the current functionality-first scope and implementation priority so deferred systems do not drift back into the critical path.

## Implementation priority

1. **Hold/Stop** — basic explicit stop only. `H` is the current test binding because native `S` pauses match presentation.
2. **Auto-attack chase timing** — observe vanilla AI chase/attack recovery and mirror the game's own movement-resume cadence instead of inventing an orb-walk timer.
3. **`End` — temporary release to AI / return to spectator**
   - release only the currently controlled champion;
   - restore normal AI control immediately;
   - do **not** release or accelerate pre-simulation;
   - `Ctrl+End` remains the stronger/global release.
4. **`A` — attack-move** — important ranged-character micro and especially important to Gunfighter.
5. **Pings/team commands** — potentially large subsystem; do after the core direct-control command vocabulary is stable.
6. **Shop control** — automatic shop remains acceptable until this stage.

## Must iron out before release

- **Locked-skill safety:** never probe or emit Q/W/R inputs before the slot is unlocked for the selected champion level. Current progression gate: Q level 1, W level 3, R level 5. Physical testing found that probing a locked skill could wedge the watched simulation worker; the current build now rejects locked slots before validation and has passed the reproduction test.
- **Auto-attack chase recovery timing:** do not invent our own orb-walk/reset cadence. Observe vanilla AI simply chasing a moving enemy and basic-attacking, identify when vanilla resumes movement after an attack, and mirror that timing/cancel window. The current conservative full-cooldown hold is visibly too slow.
- **Dynamic skill targeting presentation:** max-range radials/ray clipping must reflect the current live match values after simulated balance patches. Never hard-code per-champion range values from one patch.
- **Basic Return behavior:** B uses the game's native Return Home input. Current repeated Return input immediately restarts recall after damage interruption; acceptable for functionality, but channel/restart behavior is a polish candidate before final release if it remains visually or mechanically undesirable.

## Current Hold/Stop behavior

`H` cancels the selected champion's current move, exact-target attack, Return Home order, and armed skill targeting. The champion remains under authoritative manual ownership and therefore falls into the existing neutral hold-at-current-position behavior. No retaliation or autonomous target acquisition is attached to Hold.

## Deferred / later polish

- **Friendly champion selection cleanup:** do not redesign selection here. F1-F10 is sufficient for functionality; other mods can restrict scope or presentation.
- **Gunfighter move-while-attacking composition:** character-specific follow-up after generic controls are stable; attack-move is particularly important to this character.
- **Idle retaliation:** possible later behavior where an otherwise-idle selected champion that is attacked by an enemy already in legal basic-attack range returns fire.
- **Native Shortcuts-menu integration:** desirable final UX, but stable API currently provides no trivial registration hook.
- **Morgard/ping override investigation:** revisit only if explicit manual orders are still observably overridden after the core command path is stable.
