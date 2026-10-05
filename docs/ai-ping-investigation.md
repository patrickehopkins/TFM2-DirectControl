# AI-responsive ping / Athlete-stat investigation

Status: **active collaborative research handoff, October 5, 2026.**

## Why this investigation changed

Firkin's Control experiment established an important behavioral fact: player ping emission and AI command handoff are possible, but a hard `fight` command can make AI teammates **too cooperative**. In reported testing, AI would obey strongly enough to commit under tower rather than preserve its ordinary caution.

That changes the useful problem from:

```text
Can a player ping reach AI?
```

to:

```text
Can a player ping bias an existing native AI decision
without replacing vanilla danger / positioning / target judgement?
```

The preferred architecture is therefore:

```text
player ping / call
    -> temporary bias to an existing native evaluator
    -> vanilla danger / positioning / target selection still runs
    -> AI becomes more or less receptive rather than receiving an absolute order
```

## Confirmed Athlete-stat access

Earlier Flame Simulator probes on TFM2 v0.5.8 successfully resolved match players back to management-side Athlete records and read the Athlete `stat` field while a match was active.

The stable record-access pattern is conceptually:

```rust
ctx.record_get_json(
    RecordKindV1::Athlete,
    athlete_id,
    "stat",
)
```

Harbinger currently uses the same record layer for other Athlete fields such as `contract`.

The twelve numeric Athlete stat fields observed by the old probe were:

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

The in-match player-detail UI also exposes corresponding stat labels/values, including `judgement`, `mental`, `concentration`, `order`, `roaming`, `aggressive`, and `ego`. This establishes that these values remain addressable/displayable while a match is running.

## What this does **not** prove

Do not infer that live champion AI reads the management-side `Athlete.stat` object directly every decision tick.

The simulation may instead:

1. read those records live;
2. snapshot them when the simulation is created;
3. copy them into per-player/per-simulation state;
4. transform them into hidden behavioral coefficients;
5. use only selected fields in selected evaluators.

The old probe did **not** expose a native call accept/reject decision, did **not** prove an Ego/Order/Judgement roll, and did **not** identify a stat named `calls`. Any earlier shorthand such as "Ego/Calls" was conceptual language, not an observed field name.

## Existing probe neighborhood

The old Flame probe observed relevant systems concurrently:

- `ai_input`
- `player_sample`
- macro `strategy`
- `athlete_stats`

One capture recorded roughly 328k AI-input observations, 39k player samples, 78 Athlete-stat observations, and 12 strategy observations. The instrumentation was therefore already near the useful systems; it simply was not designed to trace stat-to-decision causality.

Macro strategy should not be treated as equivalent to moment-to-moment teammate calls. Previous probe evidence kept those systems distinct.

## Highest-value next experiment

Start with these fields because their names make them plausible behavioral inputs:

```text
order
ego
judgement
aggressive
roaming
```

Their actual semantics remain unproven.

Suggested investigation:

1. Resolve a known live match player to `athlete_id`.
2. Read `Athlete.stat`.
3. Trace reads, copies, or transformations of the five priority fields into match/simulation state.
4. Determine whether the values are live, snapshotted at match start, or converted into derived coefficients.
5. Locate a native evaluator equivalent to:
   - should I join this fight?
   - should I continue this fight?
   - should I rotate toward this ally/location?
   - should I respond to this teammate call?
6. Run controlled extreme-value tests (for example 0 vs 100) while holding the game situation as constant as practical.
7. If a relevant evaluator is found, test a **temporary bias to its input/score**, not replacement of the final command.

A null result from mutating `Athlete.stat` live is not enough to reject the hypothesis. If nothing changes, trace initialization/copying first; the simulation may have already cached or transformed the values.

## Desired deliverable

The useful output is a reusable hook/entry-point map, not necessarily a finished Harbinger feature:

- relevant function/class/state;
- where Athlete-derived behavior enters the simulation;
- evaluator inputs and output;
- when it runs;
- safe bias range or mechanism;
- whether the game still performs native danger/positioning/target checks;
- version/build fingerprint;
- minimal proof-of-concept if practical;
- failure modes and unproven assumptions.

## Design constraint

Do **not** implement player pings by permanently changing pre-game macro strategy. Persistent objective/tower/Morgard strategy settings are long-lived plans and are not equivalent to a temporary "consider fighting/rotating here" call.

The target is a resilient, moment-to-moment AI steering surface that can be reused by Harbinger, Control, or other mods.
