# Release-week plan

**Authoritative release cut: 2026-09-22**

This file is the short recovery document for getting Direct Control onto Steam Workshop this week. If another chat or contributor needs to reconstruct priorities, use this file before the older long-form roadmap in `release-scope.md`.

## Goal

Ship the first public Direct Control release this week without reopening the scope every time another polish idea appears.

The current Teamfight Manager 2 v0.6.1 compatibility port is physically validated: the game launches, real-time pacing works, direct champion control works, MMB camera drag works, and wheel zoom works.

## Immediate buglist before release

Do these in order unless a newly discovered crash/data-corruption-level regression forces a detour:

1. **Automatic team fog-of-war on direct control**
   - When a champion is manually controlled, select that champion's actual simulation team vision.
   - Must work when controlling either side.
   - Do not synthesize configurable spectator hotkeys as the implementation.
   - **v0.6.1 physical result:** reading `camera_handler_this+0x63` remains `0` for All / Blue / Red, proving the old diagnostic sampled the wrong object.
   - **v0.6.1 static result:** the same native camera handler at `0x00C2DBE0` loads a nested owner pointer from `this+0x418` and writes `0/1/2` to `owner+0x63` for All / Blue / Red, after checking `owner+0x10 == 0`.
   - **v0.6.1 team mapping confirmed physically:** Blue = simulation team `0`; Red = simulation team `1`. Automatic fog therefore requests native mode `selected_team + 1`.
   - Current implementation applies that request after the original camera handler on the existing native camera thread and preserves the native `+0x10 == 0` guard.
   - **Physically validated on v0.6.1:** selecting Blue forces native vision `1`; selecting Red forces native vision `2`; manual spectator vision changes are overridden while a champion is controlled; `End` stops enforcement and leaves the current native fog state in place.
   - Known/non-blocking spectator loophole: native follow-selection can still follow an opposing champion and reveal that champion's position through fog. This is accepted for the first release; Direct Control is not intended as an anti-cheat layer.

2. **F-key selection mapping hardening**
   - Keep F1-F10 as the player-facing selection scheme for this release.
   - Native Follow Own / Follow Enemy actions were investigated, but they are role-oriented (top/jungle/mid/bottom/support) rather than a direct stable-athlete-id mapping. Adding a new native follow-target dependency is not justified for the first release.
   - The existing visible-card -> stable athlete-id route is now hardened to build the ten-card roster coherently, require unambiguous athlete-name matches, reject duplicate athlete assignments, cache the mapping per match, and revalidate a cached card before reuse.
   - **Physically validated on v0.6.1:** F1-F10 continued selecting the intended visible champions across both teams and remained correct through pause/resume and UI-layout changes.

3. **Click-target hitbox/selectability polish**
   - Enlarge only clickable/selectable geometry; never alter pathing or collision.
   - Give towers/final objective substantial forgiveness, champions modest forgiveness, and creeps only enough to remain usable.
   - When enlarged areas overlap, priority is **Champion > Building/Objective > Creep**.
   - The purpose is to reduce rapid RMB attacks accidentally becoming ground MoveTo orders.
   - Current implementation uses live camera scale rather than fixed simulation padding: +12 px champion, +28 px tower, +24 px other targetable objective/building-like entity, +5 px minion. Minimap commands retain exact collision geometry.
   - **Physical validation pending.**

## Input-focus safety sweep before release

The apparent "long pause released control" regression was reproduced and traced to **global raw keyboard polling**, not pause duration itself. `GetAsyncKeyState` sees keys even while another application owns focus, so using `Ctrl+End` while typing in another program can trigger Direct Control's global release in the background.

Current raw-key paths requiring the same foreground-process gate before release:

- render-thread `Ctrl+Home` start/release chord;
- render-thread `Ctrl+End` global release chord;
- render-thread `End` temporary release;
- render-thread F1-F10 champion selection;
- worker-thread `Ctrl+Home` prematch escape path, which cannot rely on `post_render` and therefore needs its own foreground-process check.

Mouse/RMB/LMB and MMB/wheel code already verifies that the TFM2 process owns the foreground window before consuming raw Win32 input. A/B/Q/W/R/B/H/Escape use the stable SDK's contextual key input rather than these global raw keyboard polls.

**Pre-release requirement:** add one shared foreground-focus test (or equivalent safe helper) to every raw keyboard path, then physically verify that Direct Control does nothing when the user presses those shortcuts while TFM2 is unfocused.

The pause/session hardening added during investigation physically passed and may remain as defensive protection, but it was not the root cause of the observed releases.

## Pregame / pre-simulation issue

Make **one bounded pre-release attempt** after the immediate buglist.

Preferred route: reuse or adapt any clean readiness/startup solution learned from the Flame Simulator pre-game probe so the player can enter the map before meaningful watched-match simulation gets ahead.

Rules for this attempt:

- do not destabilize the already-validated 60 Hz pacing architecture;
- do not reintroduce the startup hangs from holding Candidate A too early;
- prefer a real readiness boundary over arbitrary timing hacks;
- if a clean fix does not emerge from the bounded attempt, **defer the issue and ship**;
- the first public release may therefore retain a known amount of startup pre-simulation.

This issue is desired before release, but it is **not allowed to become a release blocker**.

## Diagnostic presentation cleanup

Do one final cleanup pass immediately before packaging:

- remove or disable always-on development diagnostics, probe counters, temporary native-field readouts, and log spam that a Workshop subscriber does not need;
- remove purely diagnostic cursor/world markers or debug panels that are not part of the intended player-facing control/targeting UI;
- preserve concise user-facing control feedback, targeting/range indicators that are part of gameplay, and actionable error logging;
- keep deep diagnostics in source behind an explicit development/debug switch where practical rather than deleting useful investigation tools.

This is presentation cleanup, not permission to refactor validated control systems before release.

## Packaging / Workshop release path

Workshop packaging is part of the release task, not a new engineering subsystem.

Use Teamfight Manager 2's `TFM2ModUploader.exe`:

1. Prepare/check the mod folder metadata and Workshop images.
2. Run **Build Only (No Upload)** first and inspect the staged package.
3. Confirm the compiled Direct Control DLL and intended mod files are present and source/build junk is excluded.
4. Publish the first Workshop item, initially using non-public visibility if a final subscriber smoke test is useful.
5. Preserve the generated `mod.workshop_id`; future updates must use the same item.
6. Complete a Workshop-installed smoke test, then make the item public.

The uploader already knows how to stage native Rust mods and excludes `src/`, `target/`, `Cargo.toml`, and `Cargo.lock` from the upload.

## Everything else moves post-release

After the four immediate bugs, the bounded pregame attempt, the input-focus safety sweep, and the diagnostic presentation cleanup, **stop adding pre-release scope**. Package and ship.

The following previously listed pre-release work is now post-release unless it turns into a concrete release-breaking regression during final testing:

- synchronized ordinary match speeds and controlled-champion death fast-forward;
  - release-week experiment was rejected after the native presentation speeds failed to stay usefully synchronized with the live Direct Control simulation;
  - Highlight did expose useful death/respawn behavior, but without a reliable operator-visible way to know when simulation authority has been released, changing time scale makes the control model harder to reason about;
  - preserve the proven 1x/60 Hz release baseline and revisit the entire speed system after the first public release;
  - **multiplayer invariant remains locked:** if speed features return later, every Direct Control speed-changing feature must remain non-functional in multiplayer and multiplayer must stay at 1x.
- skill cooldown/readiness HUD and explicit red unavailable feedback;
- dynamic max-range skill radii / ray clipping;
- first-class Direct Control keybind settings and shortcut-mode separation;
- playback-desync watchdog / snap-to-live hardening beyond what is required to make synchronized speed work safely;
- current-gold HUD and richer economy display;
- champion-specific compatibility sweep, including Gambler Skill 1 and Gunfighter attack-move;
- pause/menu and View Match Results Immediately safety audit;
- resolution / aspect-ratio / DPI / UI-scaling compatibility audit;
- native-hook discovery/version-resilience hardening beyond the guarded v0.6.1 profile;
- Space recenter/follow;
- screen-edge scrolling;
- idle retaliation;
- Morgard/manual-order override investigation.

## Explicit post-release feature systems

These remain intentionally outside the first public release:

- **AI-responsive pings / teammate commands.**
  - Pursue only if genuine/native AI calls can be injected or an equally resilient future-proof route is found.
  - Do not build player pings by mutating persistent pre-game macro strategy settings.

- **Manual shopping / shop control.**
  - Vanilla automatic shopping remains a supported mode.
  - Later shop work may expose next intended purchase/upgrade and additional gold needed.

## Release-stop conditions

Only stop the release train for newly discovered failures of the existing core contract: repeatable crashes/freezes attributable to Direct Control, inability to start or complete ordinary matches, broken real-time pacing, broken fundamental movement/attack/cast control, or a packaging/install failure that prevents Workshop subscribers from running the mod.

Ordinary polish gaps should be documented and moved to post-release rather than expanding the first-release scope again.
