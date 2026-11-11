# Implementation Plan

This file is the execution checklist for the Rust implementation. The design documents remain the source of truth:

- [SPEC.md](SPEC.md): specification and battle rules
- [BLUEPRINT.md](BLUEPRINT.md): approved implementation-ready design
- [gaps.md](gaps.md): closed design checklist

## Goal

Implement the first milestone: a deterministic 1v1 battle engine that uses immutable snapshots, player-facing observations and actions, and a replayable battle result payload.

## Phase 1: Core state model

**Status: Complete** (public callers cannot mutate populated snapshots; transitions create the next snapshot from an owned engine copy)

### Tasks
- [x] Define the canonical battle snapshot.
- [x] Define participant and team state structs.
- [x] Define Pokémon runtime state structs.
- [x] Define action request/response types.
- [x] Define observation payload types.
- [x] Define battle result payload types.

### Acceptance criteria
- All state is represented in Rust with clear ownership boundaries.
- Public callers cannot mutate a populated snapshot; state changes are exposed as new snapshots from transitions.
- The schema matches the accepted JSON-style contract from the blueprint.

## Phase 2: Minimal battle loop

**Status: Complete**

### Tasks
- [x] Create the battle engine runner.
- [x] Initialize a simple 1v1 battle state.
- [x] Collect participant actions per turn.
- [x] Validate actions before resolution.
- [x] Produce a next-state snapshot.

### Acceptance criteria
- The engine advances turn-by-turn from one snapshot to the next.
- No in-place mutation of shared engine state occurs during action selection.
- A fixed input set yields deterministic output.

## Phase 3: Rule resolution

**Status: Complete**

### Tasks
- [x] Implement action ordering: swaps, charged moves, fast moves.
- [x] Implement fast-move duration tracking, completion timing, and action locks.
- [x] Implement charged-move stat effects and deterministic chance rolls.
- [x] Implement end-of-turn damage resolution.
- [x] Implement energy gain timing and same-turn spending rules.
- [x] Implement shield resolution and fallback behavior.
- [x] Implement forced replacement choices after fainting; auto-select only when exactly one living bench Pokémon remains.
- [x] Implement tie-breaking and priority ordering.
- [x] Replace the temporary left-first equal-attack CMP tie-break with a seeded 50/50 RNG coin flip; test both outcomes and deterministic replay.

### Acceptance criteria
- The engine preserves accepted timing semantics.
- Energy is spendable only in the following turn.
- Swaps resolve before damage.
- Charged moves begin on the next turn and are resolved in CMP order.
- Multi-turn fast moves lock their user and grant damage/energy only on completion; a pending charged move resolves first and completes the surviving opponent's fast move.
- Shield decisions default to no-shield when invalid or missing.

## Phase 4: Replay and output contract

**Status: Complete; optional checkpoints deferred**

### Tasks
- [x] Emit structured timeline events for each turn.
- [x] Emit immutable transition notifications to optional observers after every successful transition, including no-turn forced-switch continuations; include action requests, shield decisions, emitted events, and read-only before/after snapshots.
- [x] Implement an external replay recorder that stores the initial snapshot and each transition's inputs and events once; do not retain a full timeline-bearing snapshot for every step.
- [x] Serialize a versioned replay artifact with the initial RNG state and the rules/data-catalog revision required to reproduce the battle.
- [x] Replay the recorded inputs and verify the resulting event sequence.
- [ ] (Optional) Add replay checkpoints without duplicating prior history.
- [x] Build the battle result payload.
- [x] Include winner/loser, total turns, duration, and per-side result metadata.

### Acceptance criteria
- The battle result includes the required timeline and summary fields.
- Event ordering is deterministic and reconstructs the battle state.
- Replay recording is optional and does not make the battle runner retain history.
- A replay can be verified with a compatible rules/data-catalog revision without depending on engine internals.

## Phase 5: Validation and regression tests

**Status: In progress** (unit and replay tests pass; external compatibility fixtures remain)

### Tasks
- [x] Add unit tests for move timing and shield logic.
- [x] Add tests for guaranteed, shielded, chance-based, and distinct both-target charged-move effects.
- [x] Add tests for multi-turn fast-move duration, action locks, charged completion, and faint cancellation.
- [x] Add tests for swap-before-damage semantics.
- [x] Add tests for sole-candidate auto-switch and trainer-selected replacement when multiple bench Pokémon remain.
- [x] Add tests for energy timing and same-turn spending rules.
- [x] Add tests for equal-CMP coin-flip outcomes and replay, simultaneous fast-move knockouts, both participants locked in fast moves, and switch cooldown expiry.
- [x] Test that transition observers receive successful turn and no-turn transitions once, including selector-generated action requests, and are not notified for failed transitions.
- [x] Test compact replay serialization round-trips, reproduces the recorded event stream from the initial RNG state, and rejects incompatible rules/data-catalog revisions or mismatched events.
- [x] Add observation contract validation tests.
- [ ] Compare representative battle results against accepted PvPoke compatibility fixtures (single-active fast-move coverage passes; scenario coverage remains):
  - [x] **Manual prerequisite resolved:** Use the local `/home/aaron/code/pvpoke` checkout, upstream `git@github.com:pvpoke/pvpoke.git`, commit `3ca651c7c83f3d39704f33b28ac4ca01ae10bf16` (2026-07-30), with `src/data/gamemaster.json` blob `f18467bc4ef139e47f357e05335ca0578c4c3857`. The tracked checkout is clean; its untracked `SPEC.md` is not part of the reference.
  - [x] Document the invocation and runtime: `PVPOKE_DIR=~/code/pvpoke CONTAINER_ENGINE=podman sh engine/tools/pvpoke/generate-fixture.sh <fixture.json>`; Node 20 runs in the container, so host Node/PHP are not required.
  - [x] Define the versioned fixture schema in [BLUEPRINT.md](BLUEPRINT.md#127-pvpoke-compatibility-fixture-contract): provenance, battle setup/RNG seed, ordered action and shield inputs, and expected `BattleResult`.
  - [x] Build the Node VM harness for fixtures with one active Pokémon per side; it loads the pinned PvPoke scripts and Game Master data and emits normalized expected JSON.
  - [x] Add an explicit container-based fixture-regeneration command; committed golden JSON keeps ordinary Rust tests offline and deterministic.
  - [x] Capture and commit a deterministic neutral fast-move fixture for Pidgey versus Rattata using Tackle.
  - [x] Add a Rust integration test that loads committed fixtures, runs their action inputs, and compares the complete normalized result.
  - [x] Report fixture ID and first differing JSON path on comparison failure.
  - [ ] Add single-active fixtures for charged moves with and without shields, type effectiveness, and deterministic charged effects.
  - [ ] Extend the reference adapter for multi-Pokémon teams, swaps, and faint/replacement scenarios, or record the precise PvPoke simulation-mode limitation and corresponding compatibility delta.
  - [ ] Add chance-based or equal-CMP fixtures only when the reference RNG algorithm and draw ordering can reproduce their outcomes; otherwise retain targeted seeded engine tests without claiming PvPoke parity.
  - [ ] Document each intentional compatibility delta and require an explanation and provenance update when golden outputs change.

### Acceptance criteria
- The 1v1 baseline passes the accepted compatibility tests.
- The engine remains deterministic under fixed inputs.
- All accepted behavior is captured in regression tests.

## Implementation order

1. State structs and snapshot types
2. Action and observation contracts
3. Minimal turn loop and transition runner
4. Damage, energy, and shield processing
5. Swap and faint cascade handling
6. Timeline generation and result payload
7. Deterministic regression tests

## Definition of done for milestone 1

The first milestone is complete when the engine can:

- initialize a 1v1 battle from a known snapshot
- accept move and swap decisions through the external action API
- resolve a full turn without mutating shared state in place
- produce a deterministic next snapshot and timeline
- generate a final result payload that matches the accepted contract
