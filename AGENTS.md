# Repository Documentation Split

This repository uses a small set of markdown files with distinct responsibilities. Keep each file focused on its role so design decisions, planning, and implementation details do not drift into each other.

## Canonical design documents

### SPEC.md
Purpose: project specification and design contract.

Use this file for:
- high-level goals
- system architecture principles
- battle rules and turn semantics
- player-interface contract
- compatibility targets
- success criteria

Do not use this file for implementation steps or task tracking. This file should remain stable and rule-oriented.

### BLUEPRINT.md
Purpose: implementation-ready blueprint derived from the specification.

Use this file for:
- approved runtime state schema
- data contracts
- state ownership boundaries
- engine architecture and module responsibilities
- detailed battle-resolution semantics
- validation and compatibility notes

This file should reflect the accepted design after review. It is the bridge between the rules in SPEC.md and the actual Rust work.

## Planning and execution docs

### PLAN.md
Purpose: tactical implementation plan.

Use this file for:
- ordered implementation milestones
- execution checklist for the Rust build
- phase-by-phase work breakdown
- test-first implementation sequence

This file should stay practical and incremental. It is not the place for long-form rules or design rationale.

### gaps.md
Purpose: proof that the design work has been resolved.

Use this file for:
- tracking the original design gap categories
- marking categories as resolved when the design was accepted
- confirming that open work is now implementation-level rather than unresolved product design

This file should be kept in sync with the accepted decisions in SPEC.md and BLUEPRINT.md. It is not a backlog of active implementation tasks.

## Supporting docs

### README.md
Purpose: project overview for external readers.

Use this file for:
- repo purpose
- quick start summary
- project goals
- links to the more detailed design docs

This should stay concise and user-facing.

### docs/*.md
Purpose: reference material for battle rules and external assumptions.

Use these files for:
- source material from PvP battle docs
- rule references
- external background and compatibility notes
- explanatory context for why the engine behaves a certain way

These files should be reference documents, not the source of truth for project architecture.

## Ownership rule

When making a change, place it in the file that owns that kind of information:

- rules and architecture -> SPEC.md
- approved implementation design -> BLUEPRINT.md
- sequencing and execution -> PLAN.md
- resolved design gaps -> gaps.md
- user-facing repo summary -> README.md
- supporting references -> docs/*.md

If a change would fit more than one file, prefer the narrower, more specific document and keep the larger design documents stable.
