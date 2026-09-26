# Target selection geometry (v0.1.2 maintenance polish)

> **Locally validated on Windows v0.6.1; Workshop update pending final package checks.**
> A circle is a selection aid, not a bigger native collision box or extra skill range.

## Two layers, plus overlap priority

For any visible targetable unit:
1. **Base:** the stable simulation entity's native collision radius.
2. **Outer click ring:** add the category's screen-pixel forgiveness converted to
   simulation units using the current camera scale. This ensures that clicking
   feels consistent across zoom levels.

`effective_radius = picker_base_radius + padding_px * sim_units_per_screen_pixel`

The categories in `src/control/entity_picker.rs` are:

| Category | Outer padding | Overlap priority |
| --- | ---: | --- |
| Champion | +8 px | 1 |
| Tower | +28 px | 2 |
| Other objectives / unmodified jungle monsters | +24 px | 2 |
| Stump / mushroom jungle creeps | +20 px | 2 |
| Lane minion | +5 px | 3 |
| Bee jungle creep | +9 px | 3 |

Within the same priority tier, the closest center wins; remaining ties use
smallest clickable radius and then stable entity ID. Priority is applied to
**eligible entities under the click**, not to native collision, native skill range,
damage areas or actor pathing.

### Bees

The stable API does not expose a dedicated bee-jungle-creep type. Our narrow
classifier recognizes the observed Windows v0.6.1 runtime name `bee_monster` (confirmed
in the 2026-09-24 automatic support log), plus earlier English-name aliases.
It is applied only
after champion and tower classification.

To keep bee base geometry comparable to a lane creep, the pick-only **base radius**
is capped at the smallest live friendly/enemy lane-minion radius
observed in the authoritative simulation. It never enlarges an already-small
bee. If no lane minion exists yet, the native bee base radius is provisionally
used. The bee's outer padding is intentionally **four pixels larger** than a
lane minion's (+9 px vs +5 px), following the maintainer's in-game feedback.
Neither native collision nor combat range changes.

The first v0.1.2 support log established that the actual bee runtime name is
`bee_monster`: previously every bee appeared as `Other` with native radius
15,000, explaining the oversized +24 px objective circle. This change classifies
that precise name as `Bee` for both the drawn ring and authoritative picker.
A second ordinary-play test confirmed `bee_monster:Bee:base_radius=10000` in
the automatic support log, while the stump and mushroom were correctly recorded
as their native `stump_monster` and `mushroom_monster` names at radius 15,000.
That validates the bee classification and live lane-minion base-radius cap in
the tested session. Without live lane minions, bees temporarily retain their
native 15,000 base radius and +9 px padding; native radius is never altered.

### Stump and mushroom

The confirmed native identifiers are `stump_monster` and
`mushroom_monster`. These now have their own **+20 px** outer-pick padding
instead of the generic objective's +24 px. Their native 15,000 radius and
objective-tier priority remain unchanged. Serpen, rhino, epic monsters,
towers and other unmodified types keep their previous geometry.

### Selection-ring rendering

Champion rings keep their existing 3 px line thickness and 16 segments.
All non-champion rings now use the **same 3 px thickness** and 12 segments.
This makes lane-creep and bee outlines legible without increasing click
forgiveness; 12 segments keep draw-call costs below 16-segment champions.

### RMB versus targeted Q/W/R

Previously, contextual battlefield RMB sent the current
`sim_units_per_px` to `entity_picker::pick_hostile_entity`. The targeted-skill
LMB path called the same picker with scale **zero**, so skills accepted only the
exact native collision radius, even though the outer circles on-screen were
drawn for RMB.

Both paths now send the same **live camera scale** to the same entity picker:
- RMB: `Hostile` relation, enlarged click radius, persistent exact target ID
  and native-validator-based attack/chase.
- Q/W/R entity-target skill: `Any` relation during picking, identical expanded
  radius, then the skill's **native runtime validator** decides whether this
  actor is a legal target. Existing out-of-range hostile-target chase applies
  to that exact entity if its skill's shape is credible.

`Any` here means only "candidate for target legality validation", not
"can cast on anyone." A skill that is hostile-only will not start healing a
friendly minion simply because it lies inside an enlarged ring.

Direction/position/cursorless skills are not given entity-based click
forgiveness. Minimap RMB continues using native-only geometry by design.

## Focused maintainer acceptance test

On the v0.6.1 Windows development build:
1. Pick any champion and confirm RMB still works around the outside of the
   visible outer ring, including overlap priority and held RMB.
2. Use an entity-targeted Q/W/R skill against a valid champion. Confirm LMB in
   the **outer** 8-pixel padded band actually selects that champion and either
   casts or begins the pre-existing chase when appropriate. Check that an
   ineligible target, out-of-range position cast, and ordinary directional
   skill haven't become permissive.
3. Visit a bee jungle camp with lane creeps alive. The base ring should
   remain roughly lane-creep-sized, with slightly more click forgiveness
   than a lane creep (+9 px vs +5 px).
4. Confirm the stump and mushroom rings are modestly tighter (+20 px
   rather than +24 px), while Serpen, rhino, epic monsters and towers
   retain their original padding. If a bee still has a giant circle, inspect
   `harbinger-diagnostics.log` for `visible_nonstandard_targets`. It will
   show the native name our classifier actually encountered.
5. Run `cargo fmt --check`, `cargo check`, and `cargo test`, then a live
   match smoke test before approving the functional pull request.

These changes intentionally do not adjust click aiming projection, native
collision, simulation tick behavior, skill validator, or skill casts for
position/direction/none targeting forms.
