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
   - The old v0.6.0 native vision byte at camera/view `+0x63` is **not assumed valid on v0.6.1**; revalidate or find the new native state before writing anything.
   - `End` should return the champion to AI/spectator without forcibly changing fog unless we explicitly choose that behavior later.

2. **F-key selection mapping hardening**
   - Keep F1-F10 as the player-facing selection scheme for this release.
   - Verify native role ordering and investigate piggy-backing the game's Follow Own / Follow Enemy role tracking.
   - A/B test any native mapping against the currently working UI-card/name mapper.
   - Keep the existing mapper if the native route is less reliable.

3. **Synchronized match speeds + death fast-forward**
   - Presentation rate and Candidate-A simulation pacing must change together.
   - Preserve the requested ordinary rates where practical: 0.5x/1x/1.5x/2x/3x mapped to approximately 30/60/90/120/180 simulation ticks per wall-clock second.
   - Re-anchor pacing immediately whenever the rate changes.
   - Do not allow ordinary replay seeking/highlight behavior to separate presentation from the live simulation.
   - Preferred Highlight replacement: while the controlled champion is dead, temporarily fast-forward both clocks together, then restore the previous ordinary speed on respawn, release, or selection of a living champion.

4. **Click-target hitbox/selectability polish**
   - Enlarge only clickable/selectable geometry; never alter pathing or collision.
   - Give towers/final objective substantial forgiveness, champions modest forgiveness, and creeps only enough to remain usable.
   - When enlarged areas overlap, priority is **Champion > Building/Objective > Creep**.
   - The purpose is to reduce rapid RMB attacks accidentally becoming ground MoveTo orders.

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

After the four immediate bugs and the bounded pregame attempt, **stop adding pre-release scope**. Package and ship.

The following previously listed pre-release work is now post-release unless it turns into a concrete release-breaking regression during final testing:

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
