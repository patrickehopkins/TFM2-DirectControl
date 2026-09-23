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
   - Current implementation uses live camera scale rather than fixed simulation padding: +8 px champion, +28 px tower, +24 px other targetable objective/building-like entity, +5 px minion. Minimap commands retain exact collision geometry. Champion padding was reduced from +12 px after physical play showed champions could crowd out minions directly beneath them.
   - **Physically validated on v0.6.1:** the enlarged regions and Champion > Building/Objective > Creep overlap priority behave as intended. The optional visual hitbox rings remain lightweight by throttling geometry snapshots and using low-segment outlines; full-rate targeting itself is unchanged.

## Input-focus safety sweep before release — validated

The apparent "long pause released control" regression was reproduced and traced to **global raw keyboard polling**, not pause duration itself. `GetAsyncKeyState` sees keys even while another application owns focus, so using `Ctrl+End` while typing in another program can trigger Direct Control's global release in the background.

Current raw-key paths requiring the same foreground-process gate before release:

- render-thread `Ctrl+Home` start/release chord;
- render-thread `Ctrl+End` global release chord;
- render-thread `End` temporary release;
- render-thread F1-F10 champion selection;
- worker-thread `Ctrl+Home` prematch escape path, which cannot rely on `post_render` and therefore needs its own foreground-process check.

Mouse/RMB/LMB and MMB/wheel code already verifies that the TFM2 process owns the foreground window before consuming raw Win32 input. A/B/Q/W/R/B/H/Escape use the stable SDK's contextual key input rather than these global raw keyboard polls.

**Physically validated on v0.6.1:** one shared foreground-process gate now protects every raw-key path listed above. Ctrl+Home, Ctrl+End, End, F1-F10, and the worker-thread startup escape do nothing while TFM2 is unfocused. Shortcuts held while focus returns are swallowed until released and pressed again, preventing a background chord from becoming a synthetic in-game rising edge.

The pause/session hardening added during investigation remains as defensive protection, but it was not the root cause of the observed releases.

## Pregame / pre-simulation issue — validated release solution

Physical probing on v0.6.1 established that Candidate A is the watched `ClientMatchView` simulation from tick 1, but TFM2 synchronously requires meaningful simulation progress before the battlefield can be constructed. Zero-tick and one-tick holds therefore remain rejected.

The accepted release path is:

- use the bounded startup hold plus 60 Hz loader runway so the battlefield can become interactive;
- freeze Candidate A at the first usable `InGame` boundary;
- keep Direct Control locked while the visible presentation catches the frozen live simulation;
- show **“Synchronizing Direct Control with the live match...”** during catch-up;
- once synchronized, show **“Direct Control is ready. Press Ctrl+Home to take control and resume the match.”**;
- reject `Ctrl+Home` until synchronization is complete.

Physical validation passed normal startup, presentation catch-up, `Ctrl+Home` resume, selection, movement, attack, skills, MMB/zoom, and pause/resume.

The release still contains unavoidable loader-required pre-simulation. Do not attempt to hide it by shifting global spawn/AI schedules around an assumed fixed number of seconds; tests showed the readiness tick/time varies with startup behavior.


## Final control polish before cleanup

**Held-RMB command refresh — validated**
- RMB press still issues the existing contextual command immediately.
- While RMB remains physically held, continuously publish the current cursor through the existing contextual RMB path.
- The authoritative 60 Hz simulation callback remains responsible for deciding Attack(entity) vs MoveTo(point).
- Moving the cursor onto, off, or among hostile entities while RMB remains held therefore updates context live.
- No copied external repeat interval, native hook, or alternate attack path was required.
- **Physically validated on v0.6.1:** held RMB feels correct with no noted regressions; single-click contextual behavior remains intact.

**Champion click forgiveness revisit — validated**
- champion click padding reduced from +12 px to +8 px because the larger region made minions directly under champions harder to select;
- tower/objective/minion padding is unchanged;
- the visual champion circle uses the same effective radius and therefore shrinks automatically with the clickable region;
- **physically accepted on v0.6.1:** the reduction is less intrusive while preserving useful champion click forgiveness.

**Self-only skill auto-cast — test build**
- pressing Q/W/R should be the complete command for a skill that is genuinely cursorless/self-only;
- validator-accepted `TargetKind::None` skills cast immediately on key press;
- vanilla self buffs encoded as Targeting + AllyOnlySelf also cast immediately when self is legal, all other currently visible entity targets are illegal, and Direction/Position forms reject;
- ordinary target/position/direction skills retain the existing aim/click flow;
- cooldown and locked-slot safety gates remain unchanged;
- Berserker is the benchmark because his self attack steroid is core to his play pattern;
- physical validation pending.

Space recenter/follow is now deliberately post-release. Its complete investigation history, along with other worked-but-deferred systems, is preserved in `docs/deferred-investigations.md`.

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

After self-only skill auto-cast validation and diagnostic presentation cleanup, **stop adding pre-release scope**. Package and ship.

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
