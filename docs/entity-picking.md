# Cursor entity picking

Status: generic picker implemented; runtime presentation-time validation intentionally deferred until the watched simulation can be sampled at or near the displayed tick.

## Problem

TFM2's battlefield units are simulation entities, not UI `button` / `selectable` nodes. The client UI click events therefore are not a useful per-unit hit-test surface for MOBA-style controls.

The stable simulation API already provides the geometry and identity required for a generic picker:

- `entity_count()` / `entity_at(index)`
- `id()`
- `pos() -> (u64, u64)`
- `radius()`
- `team()`
- `is_alive()`
- `is_targetable()`
- `is_champion()` / `is_tower()` / `is_minion()`

The existing camera work has separately validated:

```text
Win32 cursor
    -> logical UI coordinate
    -> battlefield viewport gate
    -> live camera projection
    -> stable simulation coordinate
```

Together, these two surfaces eliminate the need for hand-authored click boxes for individual champions, creeps, towers, or future targetable entity types.

## Picker rule

`src/entity_picker.rs` takes:

```text
simulation
controlled team
relation filter: Any / Friendly / Hostile
click x/y in simulation coordinates
minimum mouse-pick radius
```

For every entity it:

1. rejects dead entities;
2. rejects untargetable entities;
3. applies the requested team-relation filter;
4. uses `max(entity.radius(), minimum_pick_radius)` as the effective mouse hit radius;
5. accepts entities whose center is within that radius of the click;
6. chooses the candidate with the nearest center;
7. uses smaller effective radius, then entity id, only as deterministic tie-breakers.

`pick_hostile_entity(...)` is the convenience wrapper intended for contextual RMB attack. The generic `pick_entity(...)` path remains available for later targeted skills that may need friendly or unrestricted selection.

## Mouse tolerance must scale with zoom

The minimum radius is intentionally separate from combat collision radius. A tiny simulation collision circle should not require pixel-perfect clicking, especially while zoomed out.

The caller should define responsiveness in screen-space pixels, then convert that tolerance through the same live camera scale used by the validated cursor projection:

```text
logical_world_units_per_ui_x = camera_extent_x / Game_map_width
logical_world_units_per_ui_y = camera_extent_y / Game_map_height

simulation_units_per_ui_x = logical_world_units_per_ui_x * 1000
simulation_units_per_ui_y = logical_world_units_per_ui_y * 1000

minimum_pick_radius = desired_click_radius_pixels
                    * representative_simulation_units_per_ui_pixel
```

Using the larger X/Y scale is the conservative choice if they ever differ. At normal square camera extents they should be effectively equal.

This keeps the extra mouse forgiveness approximately constant on screen as zoom changes, while the entity's own collision radius still provides its natural world-space footprint. The exact pixel tolerance is a feel/tuning value and should be chosen during physical testing rather than hard-coded from theory.

## Why not per-character hitboxes

Per-character hitboxes would duplicate information the simulation already owns and would become a maintenance problem whenever:

- a new champion is added;
- a mod adds a unit;
- a summon or creep type appears;
- collision radius changes;
- a unit becomes temporarily untargetable.

The generic picker follows live entity state automatically.

## Command integration

Once the watched/local simulation has been paced to presentation time, contextual RMB should be:

```text
RMB click
  -> project cursor to simulation x/y
  -> convert desired screen-space click tolerance to simulation units
  -> pick_hostile_entity(...)
       -> Some(entity): InputV1::action(Attack, Target(entity.id))
       -> None:         InputV1::move_to(x, y)
```

The stable API supports the target attack form with `InputKindV1::Attack` and an `InputTargetV1` of kind `Target`.

## Current validation boundary

The geometric screen-to-world projection has already passed physical testing across pan, zoom, full battlefield view, and Match Info layouts.

The generic picker is deliberately not yet connected to the current Candidate-A read-only pacing observer. The existing evidence shows that Candidate A can run far ahead of visible playback and eventually finish while the displayed match continues. Querying entities from that ahead-of-playback simulation at click time would therefore produce a technically valid entity pick from the wrong match tick.

Do not "validate" targeting by comparing the cursor against ahead-of-playback entity state. Runtime picker validation should happen when either:

1. Candidate A has been paced close enough to the displayed tick that its entity positions represent what the user sees; or
2. a separate presentation-time entity-state source is proven.

Until then, keep the picker as a pure reusable simulation-side component and do not emit attack commands from this branch.
