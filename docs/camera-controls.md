# Camera controls

## Validated behavior

Middle-mouse drag is now **physically validated** and matches the intended MOBA-style grab-and-drag behavior. Harbinger calculates a desired camera center from total cursor displacement, but moves the camera only through TFM2's native pan inputs; the game's real camera remains authoritative. This eliminated the previous flicker, snap-back, and screen-to-world disagreement caused by directly overwriting derived camera-center fields.

MMB drag itself is considered validated. Preserve its position-servo/native-pan mechanics unless a reproducible regression requires reopening them.

Minimap camera relocation remains native TFM2 behavior and composes correctly with Direct Control's contextual RMB minimap commands when MMB is not held.

## Match-wide availability

MMB drag and edge scrolling are **match-view QoL**, not champion-ownership features. They remain available throughout an interactive match whether the user is manually controlling a champion or spectating after `End` releases control.

Combat/control keybind mode may still switch between Direct Control and spectator behavior, but camera gestures must not disappear merely because no champion is selected.

## MMB pointer-ownership rule

Physical testing found one remaining integration problem: TFM2's UI pointer routing can temporarily stall camera integration while an MMB drag crosses interactive UI. The visible symptom is that camera motion pauses over a panel/minimap and then catches up after the cursor leaves that widget.

The intended rule is now explicit: **while MMB is held, MMB camera drag owns pointer routing and native match UI is non-interactive to the pointer.** In practical terms:

- native UI hover/capture must not interrupt or slow camera drag;
- the minimap, player cards, score/stat bars, and other widgets should behave as though they are not under the pointer for the duration of the MMB hold;
- Harbinger must still read the real OS cursor, so Direct Control LMB/RMB gameplay commands on battlefield ground/characters continue to use the player's true pointer location while MMB is held;
- native UI interaction is restored immediately when MMB is released;
- ordinary minimap LMB camera relocation remains unchanged when MMB is not held.

The current implementation uses a narrow match-only window-procedure shim to hide the physical MMB pointer from native UI hit testing while leaving Harbinger's Win32 cursor/button polling untouched. Do not solve this by moving the OS cursor or by writing the derived camera center directly.

## Edge-scroll completion

Edge scrolling is **not deferred**. It remains part of the current pre-release camera milestone and should be finished before moving on.

The confirmed failure pattern is highly specific:

- stationary physical edge hover advances in small/ticking increments;
- moving/wiggling the mouse while still touching the same edge produces smooth camera motion;
- top/bottom UI regions can suppress edge movement even though the OS cursor is physically at the real client edge;
- full-view and Info/split-view edge geometry can therefore appear different because different UI occupies those areas.

The current corrective approach preserves native camera authority and reproduces the good path: while the real cursor remains at an edge, Harbinger keeps the native pan input active and periodically posts a harmless synthetic mouse-move pulse at a safe battlefield coordinate. This should make stationary hover use the same continuous camera-update cadence observed when the user physically wiggles the mouse and should prevent top/bottom UI hover from gating the camera.

Required physical result:

- all four physical client edges work in both full and Info/split layouts;
- a completely stationary cursor glides continuously rather than ticking;
- top/bottom bars do not create dead camera zones;
- edge speed remains useful and consistent;
- no camera-center writes or second camera authority are reintroduced.

## Follow/recenter

A dedicated hold-to-center/follow action (for example Space) remains desirable after MMB and edge scrolling are both fully validated. Persistent follow/lock already exists natively through TFM2's F-key camera behavior and can later be exposed more cleanly through the custom shortcut pass.
