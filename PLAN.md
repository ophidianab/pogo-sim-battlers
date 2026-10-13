# Implementation Plan

This file is the execution checklist for the Rust implementation. The design documents remain the source of truth:

- [SPEC.md](SPEC.md): specification and battle rules
- [BLUEPRINT.md](BLUEPRINT.md): approved implementation-ready design
- [gaps.md](gaps.md): closed design checklist

## Goal

Implement the first milestone: a deterministic 1v1 battle engine that uses immutable snapshots, player-facing observations and actions, and a replayable battle result payload.

## Phase 1: Core state model

### Tasks
- Define the canonical immutable battle snapshot.
- Define participant and team state structs.
- Define Pokémon runtime state structs.
- Define action request/response types.
- Define observation payload types.
- Define battle result payload types.

### Acceptance criteria
- All state is represented in Rust with clear ownership boundaries.
- The snapshot is immutable by design.
- The schema matches the accepted JSON-style contract from the blueprint.

## Phase 2: Minimal battle loop

### Tasks
- Create the battle engine runner.
- Initialize a simple 1v1 battle state.
- Collect participant actions per turn.
- Validate actions before resolution.
- Produce a next-state snapshot.

### Acceptance criteria
- The engine advances turn-by-turn from one snapshot to the next.
- No in-place mutation of shared engine state occurs during action selection.
- A fixed input set yields deterministic output.

## Phase 3: Rule resolution

### Tasks
- Implement action ordering: swaps, charged moves, fast moves.
- Implement fast-move duration tracking, completion timing, and action locks.
- Implement charged-move stat effects and deterministic chance rolls.
- Implement end-of-turn damage resolution.
- Implement energy gain timing and same-turn spending rules.
- Implement shield resolution and fallback behavior.
- Implement forced replacement choices after fainting; auto-select only when exactly one living bench Pokémon remains.
- Implement tie-breaking and priority ordering.
- Replace the temporary left-first equal-attack CMP tie-break with a seeded 50/50 RNG coin flip; test both outcomes and deterministic replay.

### Acceptance criteria
- The engine preserves accepted timing semantics.
- Energy is spendable only in the following turn.
- Swaps resolve before damage.
- Charged moves begin on the next turn and are resolved in CMP order.
- Multi-turn fast moves lock their user and grant damage/energy only on completion; a pending charged move resolves first and completes the surviving opponent's fast move.
- Shield decisions default to no-shield when invalid or missing.

## Phase 4: Replay and output contract

### Tasks
- Emit structured timeline events for each turn.
- Serialize snapshots and event logs for replay/debugging.
- Build the battle result payload.
- Include winner/loser, total turns, duration, and per-side result metadata.

### Acceptance criteria
- The battle result includes the required timeline and summary fields.
- Event ordering is deterministic and reconstructs the battle state.
- Replay output can be inspected without depending on engine internals.

## Phase 5: Validation and regression tests

### Tasks
- Add unit tests for move timing and shield logic.
- Add tests for guaranteed, shielded, chance-based, and distinct both-target charged-move effects.
- Add tests for multi-turn fast-move duration, action locks, charged completion, and faint cancellation.
- Add tests for swap-before-damage semantics.
- Add tests for sole-candidate auto-switch and trainer-selected replacement when multiple bench Pokémon remain.
- Add tests for energy timing and same-turn spending rules.
- Add tests for equal-CMP coin-flip outcomes and replay, simultaneous fast-move knockouts, both participants locked in fast moves, and switch cooldown expiry.
- Add replay fidelity tests.
- Add observation contract validation tests.

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
