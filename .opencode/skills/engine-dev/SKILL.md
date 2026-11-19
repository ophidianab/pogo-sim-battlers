---
name: Engine Development
description: Build, test, lint, and regenerate PvPoke fixtures for the Rust battle engine
---

# Engine development

The crate is in `engine/`; there is no root `Cargo.toml`. Run commands from the
repository root through `just`, or pass `--manifest-path engine/Cargo.toml`.

## Feedback loop

```sh
just check       # fmt-check + clippy -D warnings + all tests
just test        # cargo test --all-targets (64 property cases)
just props       # property tests only (64 cases)
just props-thorough  # 1024 property cases, for rule changes
just compat      # only the PvPoke compatibility fixtures
just lint        # cargo clippy --all-targets -- -D warnings
just fmt         # cargo fmt --all
just fmt-check   # cargo fmt --all -- --check
```

Property cases default to 64 locally so `just check` stays fast; set
`PROPTEST_CASES` to raise it, or run `just props-thorough`. CI runs 512 cases on
every push, and persisted regression seeds replay in every mode.

`engine/tests/properties.rs` holds the `proptest` cases: determinism, replay
round-trip, snapshot JSON round-trip, per-turn state invariants, and the
finished-battle result contract. Add new rules invariants there rather than as
one-off example tests.

A change is not complete until `just check` passes.

## PvPoke fixtures

Regenerate one committed fixture with:

```sh
just fixture engine/tests/fixtures/pvpoke/<fixture>.json
```

The script requires the pinned checkout at `~/code/pvpoke` (override with
`PVPOKE_DIR`) and uses the Node 20 container through `podman` by default
(override with `CONTAINER_ENGINE`). It refuses to run when PvPoke `src/` has
local changes, because fixtures must record provenance from a clean,
reproducible reference commit.

Do not hand-edit golden fixture outputs. Regenerate them, then run
`just compat`. If output changes, explain the compatibility delta and update
the provenance documented in `PLAN.md` and `BLUEPRINT.md`.

## Invariants

- Battles are deterministic for a fixed snapshot, action list, and RNG seed.
- Transitions never mutate a shared snapshot; they produce the next snapshot.
- Energy is spendable only on the following turn.
- Swaps resolve before damage.
- Multi-turn fast moves grant damage and energy only on completion.
- Invalid or missing shield decisions default to no shield.
- Preserve the public snapshot JSON contract unless a design document
  explicitly changes it.

## Documentation ownership

Rules and architecture belong in `SPEC.md`; approved implementation design in
`BLUEPRINT.md`; sequencing and checkboxes in `PLAN.md`; resolved design gaps in
`gaps.md`. Do not put implementation tracking into `SPEC.md` or `BLUEPRINT.md`.
