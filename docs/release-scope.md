# Direct Control release scope

This file records the current functionality-first scope and the items that must not be accidentally promoted back into the critical path.

## Must iron out before release

- **Locked-skill safety:** never probe or emit Q/W/R inputs before the slot is unlocked for the selected champion level. Current progression gate: Q level 1, W level 3, R level 5. This exists because physical testing found that probing a locked skill can wedge the watched simulation worker.
- **Auto-attack chase recovery timing:** do not invent our own orb-walk/reset cadence. Observe vanilla AI simply chasing a moving enemy and basic-attacking, identify when vanilla resumes movement after an attack, and mirror that timing/cancel window. The current conservative full-cooldown hold is visibly too slow.
- **Dynamic skill targeting presentation:** max-range radials/ray clipping must reflect the current live match values after simulated balance patches. Never hard-code per-champion range values from one patch.
- **Basic Return behavior:** B uses the game's native Return Home input. Current repeated Return input immediately restarts recall after damage interruption; acceptable for functionality, but channel/restart behavior is a polish candidate before final release if it remains visually or mechanically undesirable.

## Minimal functionality still worth adding

- **Hold/Stop:** only the basic explicit command is required. Do not turn it into a large behavior project.

## Deferred beyond the current release-critical path

- **Friendly champion selection cleanup:** do not redesign selection here. F1-F10 is sufficient for functionality; other mods can restrict scope or presentation.
- **Pings/team commands:** potentially large subsystem; defer rather than block 1.0 functionality work.
- **Shop control:** automatic shop remains acceptable.
- **Attack-move:** defer for now. It is important ranged-character micro and especially important to Gunfighter, but it is a separate behavior layer from the current explicit move/attack controls.
- **Gunfighter move-while-attacking composition:** character-specific follow-up after generic controls are stable.
- **Idle retaliation:** possible later behavior where an otherwise-idle selected champion that is attacked by an enemy already in legal basic-attack range returns fire.
- **Native Shortcuts-menu integration:** desirable final UX, but stable API currently provides no trivial registration hook.
- **Morgard/ping override investigation:** revisit only if explicit manual orders are still observably overridden after the core command path is stable.
