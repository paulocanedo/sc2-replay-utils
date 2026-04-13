# Supply Block Detection — Provisional Strategies

> **Status:** experimental / under analysis. The current implementation
> exposes three different start-detection strategies behind a compile-time
> toggle (`ACTIVE_STRATEGY` in `src/supply_block.rs`). None of the three
> has been validated as definitive — we keep all three to compare against
> golden replays before deciding which one (or which combination) to ship.

## Why we have multiple strategies

A "supply block" intuitively means *the player wanted to make a unit but
couldn't because supply capacity was full*. The replay tracker doesn't
emit a "blocked" event — we have to derive it from periodic stats
snapshots, production events, and structure completions. Every signal we
have is either lossy, stale, or ambiguous:

- `StatsSnapshot` is sampled roughly every ~160 loops (~7 s game time),
  so `supply_used`/`supply_made` can be stale by several seconds when an
  event of interest fires.
- `ProductionStarted` for a unit fires regardless of whether the queue
  was actually constrained by supply.
- Some morphs (Lair/Hive/Orbital/PF/OverlordTransport) **don't** change
  supply capacity; we can't blindly count every "structure finished".
- Orbital Command's `SupplyDrop` (Calldown Extra Supplies) raises the
  cap without any structure event firing.

Each strategy below trades off precision and recall differently.

## The three strategies

### 1. `ProductionAttempt` (currently active)

Block **starts** when a `ProductionStarted` (Unit/Worker) fires *and* the
available supply (`supply_made − supply_used`) at that moment is less
than the unit's supply cost (looked up via `balance_data::supply_cost_x10`).

- Pros: matches the intuitive definition almost exactly. Doesn't false-
  positive when a player simply sits at 200/200 with nothing queued.
- Cons: depends on the unit-cost lookup being complete; ignores blocks
  that happen between snapshots without any production attempt being
  recorded.

### 2. `CompletedSupplyCap`

Maintains an internal `completed_supply_used` counter (incremented on
`ProductionFinished` for Units/Workers, decremented on `Died`). Block
starts when that counter reaches the current capacity.

- Pros: doesn't need the unit-cost lookup at start time; straightforward
  invariant ("you maxed out").
- Cons: false-positive at 200/200 idle; depends on having complete birth
  history (early replays where some units are already alive at game
  start can drift).

### 3. `TotalSupplyCap`

Like `CompletedSupplyCap`, but units **in production** also count toward
the supply usage (incremented on `ProductionStarted`, decremented on
`ProductionCancelled` or `ProductionFinished` failure paths).

- Pros: catches the "queued five marines but only have supply for three"
  case earlier than `CompletedSupplyCap`.
- Cons: more aggressive — over-counts when a queue is intentionally
  staged ahead of an Overlord/Pylon that's about to finish.

## Block end (shared across strategies)

End detection is the same for all three strategies (see `supply_freed()`
in `src/supply_block.rs`). A block ends as soon as the active "used"
metric drops below the current "made":

| Strategy             | "used" metric checked       |
|---------------------|-----------------------------|
| `ProductionAttempt` | `supply_used` (snapshot)    |
| `CompletedSupplyCap`| `completed_supply_used`     |
| `TotalSupplyCap`    | `total_supply_used`         |

End-triggering events:
- `SupplyReady` — finished structure (`SupplyDepot`, `Pylon`, `Overlord`,
  `CommandCenter`, `Nexus`, `Hatchery`) **or** an Orbital `SupplyDrop`
  (the Calldown adds 8 supply).
- `UnitDied` — a Unit/Worker died, releasing its supply cost.
- `ProductionCancel` — only relevant for `TotalSupplyCap`, frees the
  reserved supply.

`SupplyReady` events also patch `last_supply_made` directly, which fixes
a stale-snapshot class of false positives where a `SupplyDrop` raises
the cap mid-snapshot-interval and the next stats sample doesn't arrive
for several seconds.

## How to switch

In `src/supply_block.rs`:

```rust
const ACTIVE_STRATEGY: StartStrategy = StartStrategy::ProductionAttempt;
```

Recompile. The `StartStrategy` enum and per-strategy bookkeeping stay
compiled in regardless — the toggle is intentionally cheap so reviewers
can A/B against the same replay quickly.

## What still needs deciding

- Which strategy (or hybrid) becomes the shipped default.
- Whether to expose the toggle to end users (settings UI) or hard-code
  the chosen strategy.
- Whether to drop the `200/200` guard once the strategy stops false-
  positiving on max-supply idle.
- Whether to merge nearby blocks (< N loops apart) into one for display.

Until those are answered, the three-way toggle stays.
