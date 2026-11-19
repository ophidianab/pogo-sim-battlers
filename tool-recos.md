# Tooling recommendations for pogo-sim-battlers

*Advisory snapshot: 2026-11-19, based on the repository at commit `904d6ec` and the host environment described below. This supersedes the 2026-11-11 snapshot (commit `0366645`); see [What changed since the last snapshot](#what-changed-since-the-last-snapshot). This is an advisory reference, not a canonical design document; it does not change the ownership rules in `AGENTS.md`. Tool capabilities and versions can change, so re-check the links in [Appendix A](#appendix-a-tool-reference) before relying on them.*

## Context

This document is about tools that make an agent (and a human) more effective in this repository. It is not about adding product features. Recommendations are ordered by expected impact.

### What changed since the last snapshot

The 2026-11-11 assessment opened with a blocker: the host had `rustc` but no `cargo`, so the crate could not be built or tested. That is resolved, and most of that snapshot's recommendations are now implemented.

| Recommendation | Status on 2026-11-19 |
| --- | --- |
| Host Rust toolchain | **Done.** `cargo`, `clippy`, `rustfmt`, and `rust-analyzer` are installed from Fedora `1.99.1` packages. `rustup` is still absent. |
| `rust-toolchain.toml` | **Done.** Pins `1.99` for rustup users and CI; the local distro `cargo` ignores it. |
| `opencode.jsonc` | **Done.** LSP, formatter, and the `pvpoke` reference are enabled and loaded. |
| `justfile` | **Done.** `check`, `test`, `props`, `props-thorough`, `compat`, `lint`, `fmt`, `fmt-check`, and `fixture` exist. |
| `.opencode/skills/engine-dev/SKILL.md` | **Done.** Documents the command surface and engine invariants. |
| GitHub Actions CI | **Done and green.** The first run, `44462867971`, passed in about 50 seconds. |
| `proptest` | **Adopted.** Two properties plus tracked regression seeds; 64 local cases, 512 in CI. |
| `cargo-nextest`, `cargo-mutants`, coverage | Still not installed. |
| Beads, CodeGraph, a hook framework | Still deferred. |

The crate also grew from roughly 5.5k to 6.3k lines (5.4k in `engine/src`), and property testing added a dev-dependency tree. That shifts the remaining recommendations from environment setup toward **test throughput**, **rule-quality checks**, and **CI maintenance**.

### Environment snapshot

| Check | Result | Implication |
| --- | --- | --- |
| `rustc` | 1.99.1 (Fedora 1.99.1-2.fc44) | Matches the CI pin. |
| `cargo` | 1.99.1 (Fedora) | The crate builds and tests from the host. |
| `clippy` | 0.1.99 | The lint gate works locally. |
| `rustfmt` | 1.10.0 | Formatting and `formatter: true` work. |
| `rust-analyzer` | 1.99.1 | LSP diagnostics are available. |
| `rustup` | not found | Distro toolchain; `rust-toolchain.toml` is advisory locally and enforced in CI. |
| `just` | 1.59.0 | One command surface for the feedback loop. |
| `gh` | 2.103.3, authenticated as `ophidianab` | CI/PR inspection works. |
| CI | `.github/workflows/ci.yml`; first run green in ~50s | A second validation signal now exists. |
| CI action versions | `checkout@v4`, `cache@v4`, `upload-artifact@v4` | GitHub reports a Node.js 20 deprecation warning; current majors are v7, v6, and v7. |
| `proptest` | 1.12.0; 2 properties; tracked regression seed | Local default 64 cases; CI uses 512. |
| Repo size | 6,310 lines across 10 `src/` files and 3 integration test files | Still cheap to read; a code graph is not justified. |
| Fixtures | 3 committed PvPoke golden fixtures | The custom compatibility harness is unchanged. |
| `podman` / `docker` | podman 5.8.9 rootless works; Docker 29.9.2 CLI present but daemon unavailable | Fixture regeneration uses podman. |
| Host `node` | not installed | Not required; the fixture script uses a Node container. |
| `jq` | 1.8.1 | JSON fixture inspection works. |
| `cargo-nextest`, `cargo-mutants`, `cargo-llvm-cov` | not installed | Optional next additions. |
| `bd` / `codegraph` | not installed | Still deferred. |
| Git hooks | no `core.hooksPath`; no custom hooks | Checks are manual plus CI. |
| `.gitignore` | ignores `target`, `*.rs.bk`, `**/mutants.out*` | Mutation testing was anticipated. |
| Host | 4 cores, 15 GiB RAM | Parallel test runners help a little; the property suite is CPU-bound. |
| `.cargo/config.toml` | absent | Not needed; the `justfile` owns the command aliases. |

### Decision summary

| Area | Recommendation |
| --- | --- |
| Rust toolchain | Resolved. Distro `1.99.1` locally; CI pins `1.99` through rustup. |
| LSP | Keep enabled. It provides diagnostics, not navigation; clippy and tests remain the gate. |
| Test throughput | `cargo-nextest` is the best remaining install **if** the suite grows; `just check` is about 6 seconds warm today. |
| Rule quality | Run `cargo-mutants` occasionally against `transition.rs` and `damage.rs`. Do not put it in CI yet. |
| Coverage | Use `cargo-llvm-cov` ad hoc to find untested branches, not as a required gate. |
| Beads | Conditional yes, still subject to the `PLAN.md` ownership split. |
| CodeGraph | Defer. One crate at ~6k lines is still grep-sized. |
| Hooks | Defer the `pre-commit` framework. Add two small `.githooks` scripts only if pushes start failing. |
| CI hygiene | Bump action majors to clear the Node 20 deprecation warning. |
| Best next additions | `cargo-nextest` (when needed), `cargo-mutants`, `cargo-llvm-cov`, and optional `.githooks`. |

---

## 1. The toolchain blocker is resolved

The previous snapshot's main finding no longer applies. The host now has the full loop:

```sh
just check   # fmt-check + clippy -D warnings + all tests
just test    # all tests, 64 property cases by default
just props   # only the two property tests, 64 cases
just compat  # only the PvPoke compatibility fixtures
just lint    # clippy with -D warnings
just fmt     # rustfmt in place
```

Observed timings on this 4-core host, warm build:

| Command | Time | Notes |
| --- | --- | --- |
| `just check` | ~6 s | 51 unit + 2 property + 1 compatibility + 2 snapshot tests; 64 property cases. |
| `just props` | ~4 s | The property tests dominate the suite. |
| `cargo test --all-targets` | ~20 s | No `PROPTEST_CASES` set, so proptest uses its default 256 cases. |
| `cargo test --test properties` with `PROPTEST_CASES=1024` | tens of seconds | Reserved for rule changes via `just props-thorough`. |

Two environment details are worth remembering:

- **`rust-toolchain.toml` is advisory on this machine.** The distro packages are used directly, so the file documents intent and gives CI a version to install. If you later switch to rustup, the pin takes effect automatically.
- **Use `podman`, not `docker`, for fixtures.** The Docker CLI exists but its daemon is unreachable; podman runs rootless and already has the Node 20 image cached.

The CI workflow enforces the same commands with `--locked` and `cargo +1.99`. It currently passes:

```text
gh run view 44462867971
✓ test in 50s
```

The only maintenance finding from that run is the Node.js 20 deprecation annotation on the pinned action majors (see section 5).

---

## 2. Language server support: enabled and working

The project config now turns on the built-in LSP, and `rust-analyzer` is installed. With `cargo` present, the server can run `cargo metadata` and analyze the crate.

Current project config (`opencode.jsonc`):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "lsp": true,
  "formatter": true,
  "references": {
    "pvpoke": {
      "path": "../pvpoke",
      "description": "Pinned PvPoke reference implementation and Game Master data"
    }
  }
}
```

Caveats still worth keeping in mind:

- **What LSP gives the agent is diagnostics, not navigation.** There is no `go-to-definition` or `find-references` tool in the catalog. LSP is an edit-time safety net; grep and read remain the navigation tools.
- **Clippy is the deterministic gate.** LSP diagnostics can lag or miss lints that `cargo clippy -- -D warnings` catches.
- **`lsp: true` may also start servers for non-Rust files** such as the fixture shell script. Disable individual servers if the extra processes become noisy.
- **`formatter: true` rewrites Rust files on edit.** That is safe here because `cargo fmt --check` is already clean, but do not combine it with a commit hook that auto-formats a partially staged file.

`references` remains useful because `PLAN.md` pins fixture provenance to `~/code/pvpoke`. The reference is read-only context, so it does not add a write path into the reference checkout.

---

## 3. Beads: still conditional, still not yet

Beads (`bd`) remains a Dolt-powered, dependency-aware issue tracker for agents. Nothing about the last week changes the core objection: `PLAN.md` is already the task tracker, and Beads would be a second source of truth unless you explicitly split ownership.

The repo is larger than last time but still small in task-tracking terms: one crate, three integration test files, and a single in-progress phase. Property-test failures are handled by tracked regression seeds, not by a backlog.

If you adopt it:

- Use `bd init --skip-agents` so it does not rewrite `AGENTS.md`.
- Add a short `AGENTS.md` section stating that `bd` owns granular tasks and `PLAN.md` owns milestones and acceptance criteria.
- Drive it through the CLI (`bd create`, `bd ready`, `bd update <id> --claim`, `bd close`, `bd remember`), not an MCP server.
- Decide the `.beads/` storage and git policy before init. `br` is the lighter SQLite + JSONL alternative.

**Verdict:** unchanged. Defer until there are multiple agents or sessions, many discovered tasks, or cross-milestone dependencies. The new CI and proptest workflow reduce the urgency rather than increase it.

---

## 4. CodeGraph: not yet

CodeGraph indexes the repository into a local SQLite graph and exposes it through one MCP tool. At 6,310 lines and one crate, the whole engine still fits comfortably in context, so a graph payload is more expensive than reading the relevant files.

The call graph that matters is still short: `battle → transition → damage → data`, plus `replay` and `observations`. Grep and read handle it.

Revisit when you add a CLI/UI/WASM layer, a second crate, the multi-Pokémon PvPoke adapter, or when subagents repeatedly re-discover the same structure. If you get there, Serena is still the better first try for a Rust-only repo because it is LSP-backed and offers symbol search and references without a separate index.

**Verdict:** unchanged. Defer.

---

## 5. Other tools, ranked

### P1: highest expected impact now

| Tool | Why |
| --- | --- |
| `cargo-nextest` | The property tests now dominate suite time (~4 s of a ~6 s `just check`). Nextest gives per-test timing and better isolation, which helps when a single property starts dominating. Install when the warm suite passes roughly 30 seconds. |
| `cargo-mutants` | The highest-value **correctness** addition. `transition.rs` is 2,066 lines with 51 unit tests and 2 properties; mutation testing shows whether those tests actually constrain the rules. `.gitignore` already ignores `mutants.out`. |
| `cargo-llvm-cov` | Coverage is the cheapest way to find untested rule branches before writing more fixtures. Use it to prioritize Phase 5 work, not as a CI gate. |
| CI action major bumps | `actions/checkout@v4`, `actions/cache@v4`, and `actions/upload-artifact@v4` trigger the Node.js 20 deprecation warning. Current majors are v7, v6, and v7 respectively; bump them in one small change. |
| Lightweight `.githooks` (optional) | If you want earlier feedback, use checked-in scripts, not a framework: `pre-commit` runs `just fmt-check && just lint`; `pre-push` runs `just check`. |

### P2 and low priority

| Tool | Why |
| --- | --- |
| `insta` | Snapshot tests for observation/result payloads. Optional: the PvPoke fixtures are already a custom golden harness. |
| `pretty_assertions` | Readable `assert_eq!` diffs. The crate now has a dev-dependency (`proptest`), so the marginal cost is lower than before. |
| `divan` or `criterion` | Turn-resolution throughput only if bulk simulation becomes a goal. |
| `cargo-fuzz` + `arbitrary` | Once replay or fixture JSON accepts untrusted input. |
| `ast-grep` | Structural search and rewrite for mechanical refactors. Low need at this size. |
| `cargo-semver-checks` | `snapshot_public_api` suggests the public contract matters; run if the crate is published. |
| `tracing` + `tracing-subscriber` | There is no structured logging. Turn-level traces would help debug fixture mismatches. |
| `cargo-deny` / `cargo-audit` | Supply-chain checks. Runtime dependencies are only `serde` and `serde_json`, though `proptest` enlarges the dev tree. |
| `bacon` / `watchexec` | Useful for human watch loops; not needed in an agent loop. |
| `pre-commit` / `prek` / `lefthook` | Hook frameworks remain deferred. They add installation steps and duplicate `just`/CI. Revisit with multiple contributors or non-Rust assets. |

### CLI tools vs library crates

Some of these are Cargo subcommands or standalone binaries:

```sh
cargo install cargo-nextest cargo-mutants cargo-llvm-cov cargo-semver-checks cargo-deny cargo-audit
# just, gh, prek, lefthook, and ast-grep are also available from distro packages or their installers.
```

Others are library crates added to `Cargo.toml` (use `cargo add --dev` and let cargo choose current versions):

- `proptest` (already adopted)
- `insta`
- `divan` or `criterion`
- `arbitrary`
- `tracing` and `tracing-subscriber`
- `pretty_assertions`

One general principle still holds: prefer **CLI tools** over MCP servers in this repo. Each MCP server adds a tool namespace and dispatch layer; shell commands are cheaper and easier to script.

---

## 6. What remains to add

Items 1–5 from the previous snapshot are now committed:

- [x] `rust-toolchain.toml`
- [x] `opencode.jsonc`
- [x] `justfile`
- [x] `.opencode/skills/engine-dev/SKILL.md`
- [x] `.github/workflows/ci.yml`

The remaining tooling work, in order:

1. **Bump the CI action majors** to clear the Node.js 20 deprecation warning: `checkout@v7`, `cache@v6`, `upload-artifact@v7`. This is a one-file change and the only active maintenance item.
2. **Run `cargo-mutants` once** against `transition.rs` (and `damage.rs` if time allows). Fix or explicitly accept surviving mutants; do not add it to CI yet.
3. **Add `cargo-llvm-cov`** as an ad hoc coverage check to prioritize the remaining Phase 5 fixtures.
4. **Install `cargo-nextest` only when the suite grows.** Add `just nextest` at that point; today the 6-second warm loop does not justify a second runner.
5. **Optional:** add `.githooks/pre-commit` (`just fmt-check && just lint`) and `.githooks/pre-push` (`just check`), plus a `just install-hooks` recipe that sets `core.hooksPath`.
6. **Product work, not tooling:** the remaining Phase 5 compatibility fixtures (type effectiveness and deterministic charged effects) still matter more than any tool in this document.

Deliberately skipped: Beads, CodeGraph, `pre-commit`/`prek`/`lefthook`, `bacon`, `watchexec`, and heavy MCP servers. They add process or context without enough benefit at the current size.

---

## Appendix A: Tool reference

Sections 1–2 explain the config files; [Appendix B](#appendix-b-current-file-contents) has the committed versions. This appendix lists the named tools and services. Links and major versions are current as of 2026-11-19.

### Rust core

- **cargo** — Rust's build tool and package manager; compiles, tests, runs, and fetches dependencies. *Why here:* the minimum viable feedback loop, now installed on the host. [GitHub](https://github.com/rust-lang/cargo) · [Docs](https://doc.rust-lang.org/cargo/)
- **rustup** — Rust toolchain installer and version manager; installs channels, components, and targets. *Why here:* pins the toolchain in CI; not installed locally, where Fedora packages are used. [GitHub](https://github.com/rust-lang/rustup) · [Website](https://rustup.rs/)
- **clippy** — Rust linter with correctness, style, and performance checks. *Why here:* catches suspicious rule logic before tests do; runs with `-D warnings` in `just check` and CI. [GitHub](https://github.com/rust-lang/rust-clippy) · [Docs](https://doc.rust-lang.org/clippy/)
- **rustfmt** — Rust source formatter. *Why here:* keeps diffs consistent and powers `formatter: true`. [GitHub](https://github.com/rust-lang/rustfmt) · [Docs](https://rust-lang.github.io/rustfmt/)
- **rust-analyzer** — Rust language server with diagnostics, completion, navigation, and refactors. *Why here:* powers LSP diagnostics in OpenCode. [GitHub](https://github.com/rust-lang/rust-analyzer) · [Website](https://rust-analyzer.github.io/)

### Testing and correctness

- **cargo-nextest** — Next-generation test runner for Rust; faster and clearer than `cargo test`, with per-test timing. *Why here:* the next install once the property suite grows. [GitHub](https://github.com/nextest-rs/nextest) · [Website](https://nexte.st/)
- **proptest** — Property-based testing for Rust; generates inputs and shrinks failures. *Why here:* already adopted for determinism, replay equivalence, snapshot round-trips, and result contracts. [GitHub](https://github.com/proptest-rs/proptest) · [Website](https://proptest-rs.github.io/proptest/)
- **cargo-mutants** — Mutation testing: mutates code and checks whether tests catch it. *Why here:* validates that `transition.rs` tests actually constrain behavior; `.gitignore` already anticipates `mutants.out`. [GitHub](https://github.com/sourcefrog/cargo-mutants) · [Website](https://mutants.rs/)
- **insta** — Snapshot testing for Rust with inline and file snapshots and a review workflow. *Why here:* optional for observation/result payload snapshots; the PvPoke fixtures already cover cross-engine compatibility. [GitHub](https://github.com/mitsuhiko/insta) · [Website](https://insta.rs/)
- **cargo-llvm-cov** — Code coverage using LLVM source-based coverage. *Why here:* find untested rule branches before adding fixtures. [GitHub](https://github.com/taiki-e/cargo-llvm-cov) · [Docs](https://docs.rs/cargo-llvm-cov)
- **divan** — Lightweight Rust benchmarking library with a simple API and fast runs. *Why here:* turn-resolution throughput if bulk simulation becomes a goal. [GitHub](https://github.com/nvzqz/divan) · [Docs](https://docs.rs/divan)
- **criterion** — Statistics-driven Rust benchmarking with regression detection and plots. *Why here:* heavier alternative to divan for performance work. [GitHub](https://github.com/bheisler/criterion.rs) · [Book](https://bheisler.github.io/criterion.rs/book/)
- **cargo-fuzz** — Coverage-guided fuzzing for Rust. *Why here:* useful once replay or fixture JSON parsing accepts untrusted input. [GitHub](https://github.com/rust-fuzz/cargo-fuzz) · [Book](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- **arbitrary** — Trait for deriving structured fuzz inputs; used with cargo-fuzz. [GitHub](https://github.com/rust-fuzz/arbitrary) · [Docs](https://docs.rs/arbitrary)
- **pretty_assertions** — Drop-in `assert_eq!` replacement with readable diffs. *Why here:* improves unit-test failure output; the crate now has a dev-dependency, so this is a small addition. [GitHub](https://github.com/rust-pretty-assertions/rust-pretty-assertions) · [Docs](https://docs.rs/pretty_assertions)
- **cargo-semver-checks** — Lints Rust API changes for semantic-versioning breakage. *Why here:* the `snapshot_public_api` test suggests public API stability matters; run if you publish. [GitHub](https://github.com/obi1kenobi/cargo-semver-checks) · [Cargo SemVer reference](https://doc.rust-lang.org/cargo/reference/semver.html)

### Workflow and project hygiene

- **just** — Command runner with a simple `justfile` syntax. *Why here:* owns the test/lint/format/fixture commands and the local property-case defaults. [GitHub](https://github.com/casey/just) · [Website](https://just.systems/)
- **GNU Make** — Classic build and automation tool. *Why here:* zero-install alternative, no longer needed now that `just` is present. [Savannah](https://savannah.gnu.org/projects/make) · [Website](https://www.gnu.org/software/make/)
- **gh** — GitHub's official CLI for issues, pull requests, releases, and Actions. *Why here:* inspect CI runs and PR status; already installed and authenticated. [GitHub](https://github.com/cli/cli) · [Website](https://cli.github.com/)
- **GitHub Actions** — GitHub's CI/CD service. *Why here:* runs fmt, clippy, 512-case proptest, and the regression-seed guard on every push. [Product](https://github.com/features/actions) · [Docs](https://docs.github.com/actions)
- **pre-commit** — Framework for managing and running Git hooks from a YAML config. *Why here:* deferred; `just` and CI already cover the checks. [GitHub](https://github.com/pre-commit/pre-commit) · [Website](https://pre-commit.com/)
- **prek** — Fast Rust-native alternative to `pre-commit`, compatible with its config. *Why here:* the least disruptive framework option if hooks become necessary. [GitHub](https://github.com/j178/prek)
- **lefthook** — Standalone, fast Git hook manager with parallel execution. *Why here:* another lightweight hook option; still more machinery than two shell scripts. [GitHub](https://github.com/evilmartians/lefthook) · [Website](https://lefthook.dev/)
- **cargo-deny** — Cargo dependency linting for licenses, bans, advisories, and sources. [GitHub](https://github.com/EmbarkStudios/cargo-deny) · [Docs](https://embarkstudios.github.io/cargo-deny/)
- **cargo-audit** — Audits `Cargo.lock` against the RustSec advisory database. [GitHub](https://github.com/rustsec/rustsec) · [Website](https://rustsec.org/)
- **bacon** — Background Rust code checker that runs cargo check/clippy/test and shows results in a TUI. *Why here:* nice for humans, not needed in an agent loop. [GitHub](https://github.com/Canop/bacon) · [Website](https://dystroy.org/bacon/)
- **watchexec** — Runs commands when files change. *Why here:* general-purpose file watcher; not needed in an agent loop. [GitHub](https://github.com/watchexec/watchexec) · [Website](https://watchexec.github.io/)

### Code intelligence

- **OpenCode** — The agent harness running this session; supports configuration, LSP, formatters, MCP, skills, and references. *Why here:* the project config is the highest-leverage integration point. [GitHub](https://github.com/sst/opencode) · [Website](https://opencode.ai/)
- **LSP** — Language Server Protocol; the standard protocol between editors/agents and language servers. *Why here:* OpenCode uses it for diagnostics from rust-analyzer. [GitHub](https://github.com/microsoft/language-server-protocol) · [Website](https://microsoft.github.io/language-server-protocol/)
- **Serena** — LSP-backed MCP server for semantic code retrieval and editing, including symbol search, references, and rename. *Why here:* better fit than CodeGraph for precise Rust navigation at this size. [GitHub](https://github.com/oraios/serena) · [Website](https://oraios.github.io/serena/)
- **CodeGraph** — Pre-indexed code knowledge graph stored in SQLite and exposed as one MCP tool. *Why here:* defer until the repo is larger; see section 4. [GitHub](https://github.com/colbymchenry/codegraph) · [Docs](https://colbymchenry.github.io/codegraph/)
- **MCP** — Model Context Protocol; a standard way to expose tools and resources to agents. *Why here:* powerful, but adds context and dispatch overhead; prefer CLIs for this repo. [GitHub](https://github.com/modelcontextprotocol/modelcontextprotocol) · [Website](https://modelcontextprotocol.io/)

### Issue tracking and storage

- **Beads (`bd`)** — Dolt-powered, dependency-aware issue tracker designed for AI agents. *Why here:* candidate replacement for granular tasks in `PLAN.md`; see section 3. [GitHub](https://github.com/gastownhall/beads) · [Docs](https://beads.gascity.com/)
- **br (beads_rust)** — Independent Rust port of Beads with SQLite and JSONL storage. *Why here:* lighter alternative if Dolt feels heavy. GitHub only; no separate website. [GitHub](https://github.com/Dicklesworthstone/beads_rust)
- **Dolt** — Version-controlled SQL database with Git-like branching and merges. *Why here:* Beads' storage backend; relevant only if you adopt Beads. [GitHub](https://github.com/dolthub/dolt) · [Website](https://www.dolthub.com/)
- **SQLite** — Embedded relational database. *Why here:* CodeGraph's local index store; no separate server is needed. [GitHub](https://github.com/sqlite/sqlite) · [Website](https://www.sqlite.org/)

### Reference and environment

- **PvPoke** — Open-source Pokémon GO PvP simulator and Game Master data source. *Why here:* the compatibility fixture harness pins a local checkout at `~/code/pvpoke`. [GitHub](https://github.com/pvpoke/pvpoke) · [Website](https://pvpoke.com/)
- **Dev Containers** — Specification for reproducible container-based development environments. *Why here:* `.devcontainer` defines the Rust image and rust-analyzer settings used by your editor. [GitHub](https://github.com/devcontainers/spec) · [Website](https://containers.dev/)
- **Node.js** — JavaScript runtime used by the PvPoke fixture harness. *Why here:* the host does not need it because the script runs in a Node container. [GitHub](https://github.com/nodejs/node) · [Website](https://nodejs.org/)
- **dnf** — Fedora's package manager. *Why here:* install channel for cargo, clippy, rustfmt, rust-analyzer, just, and gh on this host. [GitHub](https://github.com/rpm-software-management/dnf5) · [Docs](https://docs.fedoraproject.org/)

---

## Appendix B: Current file contents

These mirror what is committed. Adjust versions and paths if the environment changes.

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.99"
components = ["clippy", "rustfmt", "rust-analyzer"]
profile = "minimal"
```

### `opencode.jsonc`

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "lsp": true,
  "formatter": true,
  "references": {
    "pvpoke": {
      "path": "../pvpoke",
      "description": "Pinned PvPoke reference implementation and Game Master data"
    }
  }
}
```

### `justfile`

```just
# The crate lives in engine/, not at the repository root.
cargo := "cd engine && cargo"

default:
    @just --list

# Run all unit, compatibility, snapshot, and property tests.
# Property cases default to 64 locally; raise PROPTEST_CASES for a longer run.
test:
    export PROPTEST_CASES=${PROPTEST_CASES:-64} && {{cargo}} test --all-targets

# Run only the property tests at the default local case count.
props:
    export PROPTEST_CASES=${PROPTEST_CASES:-64} && {{cargo}} test --test properties

# Long property run for rule changes; CI runs 512 cases on every push.
props-thorough:
    export PROPTEST_CASES=${PROPTEST_CASES:-1024} && {{cargo}} test --test properties

# Run only the PvPoke compatibility fixtures.
compat:
    {{cargo}} test --test pvpoke_compatibility

# Fail on any clippy warning.
lint:
    {{cargo}} clippy --all-targets -- -D warnings

# Format the crate in place.
fmt:
    {{cargo}} fmt --all

# Check formatting without changing files.
fmt-check:
    {{cargo}} fmt --all -- --check

# The full local feedback loop.
check: fmt-check lint test

# Regenerate one committed PvPoke fixture from the pinned local checkout.
fixture file:
    PVPOKE_DIR="${PVPOKE_DIR:-$HOME/code/pvpoke}" \
    CONTAINER_ENGINE="${CONTAINER_ENGINE:-podman}" \
    sh engine/tools/pvpoke/generate-fixture.sh {{quote(file)}}
```

### `.github/workflows/ci.yml`

```yaml
name: ci

on:
  push:
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    defaults:
      run:
        working-directory: engine

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust 1.99
        run: rustup toolchain install 1.99 --profile minimal --component clippy --component rustfmt

      # Proptest pulls in a sizeable dev-dependency tree, so cache the build.
      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            engine/target
          key: ${{ runner.os }}-cargo-${{ hashFiles('engine/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-

      - name: Check formatting
        run: cargo +1.99 fmt --all -- --check

      - name: Lint
        run: cargo +1.99 clippy --all-targets --locked -- -D warnings

      # --all-targets includes tests/properties.rs, so the proptest cases run here.
      # CI samples 512 cases per property; the local default is 64 (see justfile).
      - name: Test
        id: test
        env:
          PROPTEST_CASES: "512"
        run: cargo +1.99 test --all-targets --locked

      # A passing run must not leave newly discovered proptest seeds uncommitted.
      - name: Check for uncommitted proptest regressions
        run: git diff --exit-code -- tests/properties.proptest-regressions

      # If a property test fails, preserve the seed it wrote for the next fix.
      - name: Upload proptest regressions
        if: failure() && steps.test.outcome == 'failure'
        uses: actions/upload-artifact@v4
        with:
          name: proptest-regressions
          path: engine/tests/properties.proptest-regressions
          if-no-files-found: ignore
```

### `.cargo/config.toml` (not needed while `just` exists)

```toml
[alias]
t = "test --all-targets"
lint = "clippy --all-targets -- -D warnings"
fmt-check = "fmt --all -- --check"
```
