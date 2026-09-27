# Known issues and deferred polish

This file describes the limitations that still apply to the first Workshop release. Completed investigations that are no longer active release issues live in `docs/deferred-investigations.md`.

## Startup requires bounded pre-simulation

Status: **accepted first-release limitation with validated synchronization.**

Teamfight Manager 2 requires watched-match simulation progress before it can construct the battlefield. Candidate A is already the correct `ClientMatchView` simulation at tick 1, but freezing it there prevents the loader from reaching a usable match view.

The release path therefore allows the validated loader runway, freezes the live simulation at the first usable `InGame` boundary, waits for visible presentation to catch the frozen live clock, and only then enables the explicit `Ctrl+Home` start. The player is never intentionally given control of simulation state that presentation has not caught up to.

True zero-pre-simulation startup remains deferred.

**Current observation (v0.6.1, maintainer's September 25 testing):** match entry
and presentation synchronization now appear effectively immediate in ordinary
use; the earlier noticeable loading delay is no longer reproducible locally.
The cause has not been established. Preserve the bounded loader runway and
sync safety checks for other machines/builds, and revisit only if new logs
show a reproducible problem.

## Enemy native follow can reveal a fogged champion

Status: **accepted first-release limitation.**

Automatic team fog follows the simulation team of the champion under Direct Control and is physically validated. TFM2's native spectator follow behavior can still follow an opposing champion and reveal that champion's position through fog.

Direct Control does not attempt to turn spectator-mode UI into an anti-cheat boundary.

## Match speed is intentionally fixed at the validated 1x baseline

Status: **post-release experiment.**

The first Workshop release keeps the live simulation on the validated 60 Hz / 1x control model. An attempted synchronized-speed system did not keep native presentation and live simulation coupled reliably enough to ship.

The investigation stopping point, including death/respawn fast-forward ideas, is preserved in `docs/deferred-investigations.md`.

## Gunfighter attack-move does not preserve his native move-while-attacking behavior

Status: **known champion-specific compatibility gap; post-release.**

Generic `A + LMB` attack-move works for ordinary champions, but Gunfighter currently resolves to either attacking or walking instead of retaining his native move-compatible attack behavior.

Future work should start from `can_use_with_move` / native move-compatible action semantics and must not change the already validated generic attack-move behavior simply to special-case Gunfighter.

## Self-only auto-cast has a conservative runtime classification

Status: **validated on known benchmarks; broader champion interactions may still need coverage.**

Berserker Skill 1 and Monk Skill 1 correctly cast immediately on keypress, while Ogre's automatic/passive trigger remained non-activatable.

The stable AI context does not expose the live base champion action definition directly, so Targeting-style self-only actions are inferred from validator evidence. If a later champion exposes an ordinary ally-target skill that is incorrectly classified as self-only in a particular situation, narrow the generic rule rather than adding broad champion hard-codes.

## Space recenter/follow is deferred

Status: **post-release.**

Multiple custom follow and native-input approaches were physically rejected, and the native follow controller proved to live upstream of the validated camera hook surface. Do not restart from custom pan chasing or synthetic F-key injection.

The full stopping point and recommended resumption routes are in `docs/deferred-investigations.md`.

## Screen-edge scrolling is deferred

Status: **post-release.**

Stationary-edge updates were visibly stepped, native UI regions interfered with the behavior, and synthetic mouse wakeups introduced flicker. MMB drag plus wheel zoom are the validated release camera controls.

See `docs/deferred-investigations.md` for the rejected approaches and resumption point.
