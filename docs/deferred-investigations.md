# Deferred investigation handoffs

This file preserves stopping points for deferred or historical investigations so later work can resume from evidence instead of repeating failed experiments. Status notes dated before the current v0.1.5 / TFM2 v0.6.2 release are historical unless a section explicitly says it is active.

## Space recenter / follow

**Status:** deferred before first Workshop release; Control v0.9 independently validates the desired UX.

October 2026 external-mod observation: Firkin's Control implements both a held center/peek action and a camera lock toggle, and its runtime logs confirm that manual panning breaks the lock. This validates Harbinger's intended interaction model but **not** Harbinger's rejected implementation routes. Control appears to use different camera/layout machinery, including minimap-derived camera-state tracking. Harbinger should continue looking for a native semantic follow/control surface rather than reviving moving-target pan or synthetic F-key injection. See `docs/control-0.9-study.md`.

**Original status:** deferred before first Workshop release.

**Desired behavior**
- Hold Space: immediately center/follow the currently controlled champion.
- Release Space: return to free camera.
- Double-tap Space: latch the same persistent follow behavior as the game's native F-key follow.
- Manual camera movement (MMB) should break a latched follow in the same way native follow does.
- Switching controlled champions while following should follow the newly controlled champion.
- No selected champion: Space has no Direct Control effect.

**Investigation branch**
- `feat/space-follow`
- Last investigation head before defer: `af53b5b51a1019e97accfaa6423419657bafa801`
- Do **not** merge this branch wholesale. It intentionally contains temporary probes and rejected experiments.

### Rejected implementation 1 — moving-target native pan

The first implementation reused the validated MMB native-pan request path and continuously drove the camera toward the selected champion's authoritative simulation position.

Physical result:
- follow visibly jerked;
- stopping after a movement order could make the camera jump ahead, rotate/circle, then settle;
- immediate centering was poor.

A second version throttled corrections to native camera callbacks, reduced gain, added a deadzone, and added double-tap lock state.

Physical result:
- still visibly jerky and not immediate.

Conclusion: MMB's pan mechanism is appropriate for a fixed drag destination, not persistent tracking of a moving simulation target. Do not revive this architecture by tuning gains again.

### Rejected implementation 2 — synthesize the native F-key

The game already exposes native role-oriented follow actions, so the next attempt translated Space into the F-key belonging to the currently controlled slot.

Two injection layers were tried:
1. posted `WM_KEYDOWN` / `WM_KEYUP` messages to the game window;
2. Windows synthetic keyboard input, with Direct Control's own F1-F10 selector masked so the injected key could not reset the player's current order.

Physical result for both:
- TFM2's native follow behavior did not respond.

Conclusion: native follow is resolved inside a deeper game/input action layer rather than ordinary posted/synthetic Windows key events.

### Native-action reverse engineering performed

The v0.6.1 executable used during the investigation:
- SHA-256: `91084e9a29c70993595a1d7d0c22064bae0ee82b0fc773c696077d15c2268f98`
- PE timestamp: `0x6AB1D950`
- image size: `0x05264000`
- known camera handler: RVA `0x00C2DBE0`

The action-name probe recovered the ten native role-follow actions as a contiguous semantic enum:
- IDs `0x1C..0x20`: own top/jungle/mid/bottom/support;
- IDs `0x21..0x25`: enemy top/jungle/mid/bottom/support;
- nearby IDs include native camera/fog/auto-follow actions.

The initial string/xref route mostly exposed enum-name/serialization plumbing rather than the live follow dispatcher.

A targeted caller probe found a small `game/view` path around `0x0086B920` that iterates roughly 55 semantic action IDs before calling the camera handler. Two notable helpers were investigated:
- `0x021DBD30`: proved to be a tiny static action-ID -> byte lookup, not a live "is action pressed?" query;
- `0x00CEB890`: looks like generic native map/update plumbing for the action-id/value pair rather than follow execution.

Do not resume by hooking either helper speculatively.

### Runtime differential probes performed

Because Direct Control already detours the native camera handler, several F1/F2 comparisons were performed while using **real** native follow:

1. First `0x430` bytes of the camera object:
   - initial F1/F2 tests found no reproducible stable follow-specific field;
   - a later F1 sample showed stable changes at offsets such as `+0x2B8`, `+0x2E8`, `+0x320`, but F2 did not reproduce them;
   - therefore these are **not verified follow fields**.

2. Six native camera-handler arguments:
   - no stable F1/F2 follow-specific value changes.

3. Small readable windows behind pointer-like handler arguments:
   - no stable reproducible F1/F2 follow-specific pointee fields.

Conclusion: native follow ownership is resolved upstream of the camera handler. The camera object receives resulting camera behavior rather than a simple stable "follow enabled + target slot" state that Direct Control can safely write.

### Best resumption point

Do **not** return to custom pan chasing, synthetic keyboard injection, or more blind camera-object scanning.

Better future routes, in order:
1. Check whether a newer SDK exposes a supported way to invoke an existing native UI/button action. The visible player cards already have native follow controls; invoking the exact native card-follow event would be preferable to input emulation.
2. Trace the upstream semantic input/controller object feeding the `game/view` camera-preparation path, focusing on live action state rather than action-name metadata.
3. If a direct native follow dispatcher/state is found, bind Space to that route and let TFM2 remain authoritative for centering, tracking, double-tap lock, and manual-pan cancellation.

**Defer rule:** if resumption again becomes a broad executable archaeology exercise without a concrete control surface, keep Space post-release.

---

## Synchronized match speed / death fast-forward

**Status:** deferred after physical release-week testing.

**Investigation branch**
- `feat/synchronized-match-speed`
- Historical head: `c3f16b5ab83492db0138b2a8b796569e9654b4e4`
- This branch diverged substantially from current main. Do **not** merge it wholesale; transplant ideas selectively.

### What was attempted

The branch explored:
- reading native presentation speed state;
- pacing Candidate A at corresponding rates;
- driving 0.5x / 1x / 1.5x / 2x / 3x;
- locking multiplayer to 1x;
- making timeline/highlight controls inert while Direct Control owns live pacing;
- death/respawn fast-forward concepts.

### Why it was deferred

Physical tests showed that native presentation speed controls and live Direct Control simulation authority did not remain reliably synchronized enough for a release-time feature. Highlight/death behavior exposed useful ideas, but without an authoritative presentation position and robust ownership indicator, variable speed made the control model less predictable.

The accepted current invariant is therefore:
- Candidate A stays at the validated 60 Hz / 1x baseline;
- startup synchronization freezes live simulation until visible presentation catches up;
- no ordinary speed-changing feature is enabled during Direct Control;
- multiplayer remains 1x if this system ever returns.

### Best resumption point

Build the future system around clocks/authority, not button state:
1. Candidate A simulation tick is the authoritative live clock.
2. Recover a precise native presentation/playback position (prefer event/controller state over whole-second UI text).
3. While Direct Control owns pacing, prevent rewind/seek/highlight actions that split presentation from live simulation.
4. Detect divergence continuously and **snap presentation directly to live**; do not repair by temporarily speeding playback.
5. Only expose variable speeds when presentation and simulation can be changed together.
6. `End` releases champion AI only; it should not restore replay freedom. `Ctrl+End` is the global pacing/replay ownership release.
7. Multiplayer stays locked at 1x/60 Hz.

---

## Screen-edge camera scrolling

**Status:** shelved; validated MMB + wheel are the release camera controls.

### What was attempted

Custom screen-edge scrolling was tested using the existing camera integration.

Observed problems:
- stationary edge hover advanced in visible ticks rather than producing smooth glide;
- top/bottom native UI regions could suppress or interfere with edge behavior;
- physical mouse motion at the edge made the native path smoother;
- attempts to manufacture that cadence with synthetic mouse movement/messages caused visible UI flicker.

### Rejected approaches

Do not restore:
- synthetic mouse wake messages;
- direct derived camera-center writes;
- cursor warping;
- workarounds that disturb native pointer/UI state.

### Best resumption point

Only revisit if a deeper native camera/update ownership route becomes available. The validated MMB architecture should remain untouched during any new investigation.

---

## AI-responsive pings / teammate calls

**Status:** active collaborative investigation; the unknown has narrowed from ping emission to safe AI-response weighting.

**October 5, 2026 collaboration update:** Firkin independently got ping emission and AI command handoff working in an experimental Control build. The failure mode was the opposite of "AI ignores me": issuing a hard `fight` command made teammates too cooperative and could make them commit suicidally under a tower. Therefore **"can a ping reach the AI?" is no longer the primary research question.**

The desired architecture is now:

```text
player ping / call
    -> temporary bias to an existing native AI evaluator
    -> vanilla danger / positioning / target judgement still runs
    -> AI becomes more or less receptive instead of receiving an absolute command
```

A separate Flame Simulator probe on TFM2 v0.5.8 established a useful lead: management-side `Athlete.stat` records were resolvable and readable while a match was active, with these twelve observed fields:

```text
last_hit
skill_avoid
skill_hit
control_speed
positioning
judgement
mental
concentration
order
roaming
aggressive
ego
```

This **does not prove** that live match AI reads those management records directly. The simulation may read them live, snapshot them at match initialization, transform them into hidden coefficients, or consult only a subset in particular evaluators. The old probe did **not** expose a native call accept/reject function and did **not** prove an Ego/Order/Judgement roll. There is no observed stat literally named `calls`.

Highest-value next probe:
1. resolve a known match player to `athlete_id`;
2. read/trace `Athlete.stat`;
3. find where `order`, `ego`, `judgement`, `aggressive`, and `roaming` enter simulation state;
4. identify a native fight-join/rotate/respond evaluator;
5. test extreme values while holding the situation as constant as practical;
6. prefer a temporary score/input bias over replacing the final AI command.

If live mutation of `Athlete.stat` has no effect, do **not** conclude the stats are irrelevant until match-initialization copies/derived coefficients have also been traced.

See `docs/ai-ping-investigation.md` for the focused technical handoff.

---

## Manual shopping

**Status:** future infrastructure/gameplay frontier, not active implementation.

Control v0.9 already has elegant player-facing current-gold and native-next-purchase presentation, closely matching Harbinger's earlier economy-HUD concept. Therefore **HUD parity is no longer a reason to pursue this work**.

The remaining Harbinger value is deeper: discover and expose the native shopping/build intent and a safe manual override/control path. Vanilla automatic shopping remains explicitly supported. If shop control is revisited, use the native intended next purchase/upgrade and live prices as research anchors, then focus on a reusable Manual Shop API rather than recreating Control's HUD.


---

## Multiplayer direct control

**Status:** reconnaissance approved; implementation not started.

The next step is to determine TFM2's native multiplayer synchronization model, not to build rollback. Start with passive two-client state/tick comparison and only then inject one harmless one-sided `InputV1` to observe whether TFM2 replicates, rejects, reconciles, or diverges.

Keep multiplayer locked to 1x / 60 Hz during all research. Use stable identity and deterministic simulation state, not presentation/UI state.

See `docs/multiplayer-research.md` for the full staged plan.
