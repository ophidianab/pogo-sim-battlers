# Tooling recommendations for pogo-sim-battlers

*Advisory snapshot: 2026-11-11, based on the repository at commit `0366645` and the host environment described below. This is an advisory reference, not a canonical design document; it does not change the ownership rules in `AGENTS.md`. Tool capabilities and URLs can change, so re-check the links in [Appendix A](#appendix-a-tool-reference) before relying on them.*

## Context

This document is about tools that make an agent (and a human) more effective in this repository. It is not about adding product features. Recommendations are ordered by expected impact.

### Environment snapshot

| Check | Result | Implication |
| --- | --- | --- |
| `rustc` | 1.99.1 (Fedora) | The compiler exists. |
| `cargo` | not found | **The crate cannot be built or tested from the host today.** |
| `clippy`, `rustfmt`, `rust-analyzer` | not found | No lint, format, or LSP feedback from the host. |
| `.devcontainer` | Rust devcontainer image, rust-analyzer extension | The editor environment is fine; OpenCode is running on the host, not in the container. |
| Repo size | ~5.5k lines across 10 source files and 2 integration tests | Small enough for grep/read; a code graph is not yet justified. |
| Fixtures | 3 committed PvPoke golden fixtures | A custom compatibility harness already exists. |
| CI | no `.github/` workflows | No second validation signal. |
| `gh` | not installed | No PR/CI/issue automation from the shell. |
| `podman` / `docker` | installed | Fixture regeneration via the containerized runtime works. |
| Host `node` | not installed | Not required; the fixture script uses a Node container. |
| `jq` | installed | JSON fixture inspection works. |
| `.gitignore` | already ignores `mutants.out` | Mutation testing has been considered before. |

### Decision summary

| Area | Recommendation |
| --- | --- |
| Rust toolchain | Install on the host, or run OpenCode inside the devcontainer. |
| LSP | Enable it; expect diagnostics, not navigation. |
| Beads | Conditional yes: only if `bd` becomes the single source of truth for granular tasks. |
| CodeGraph | Defer until the repo grows or subagents need repeated structural discovery. |
| Best next additions | `cargo-nextest`, `proptest`, `cargo-mutants`, a task runner, and CI. |

---

## 1. The blocker: install the Rust toolchain on the host

The single biggest operational gap is that **`cargo` is not available in the environment OpenCode runs in**. `rustc 1.99.1` exists, and `engine/Cargo.lock` exists, but `cargo`, `clippy`, `rustfmt`, and `rust-analyzer` do not. That means I cannot compile the crate, run the unit tests, run the PvPoke compatibility fixtures, or get lint feedback.

The `.devcontainer` uses the Rust devcontainer image and has rust-analyzer configured, so the project itself is not broken. The problem is an environment mismatch: your editor may be inside the container, while OpenCode is on the host.

### Option A: install on the host

`rustup` is the most reproducible option and lets you pin a toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add clippy rustfmt rust-analyzer
```

Fedora packages are also available and match the installed compiler:

```sh
sudo dnf install cargo clippy rustfmt rust-analyzer
```

Then add `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99"
components = ["clippy", "rustfmt", "rust-analyzer"]
profile = "minimal"
```

`rust-toolchain.toml` only takes effect through rustup. With distro packages it still documents intent and gives CI a version to enforce.

### Option B: run OpenCode inside the devcontainer

This gives the agent the exact image toolchain and avoids host drift. It is the cleanest long-term answer if you already do most development in the container, but it changes how you launch OpenCode.

### The feedback loop that matters

Once cargo exists, the loop should be:

```sh
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

That is more valuable than any single code-intelligence tool, because it is deterministic, reproducible, and directly tied to the crate.

Optional zero-dependency aliases in `.cargo/config.toml`:

```toml
[alias]
t = "test --all-targets"
lint = "clippy --all-targets -- -D warnings"
fmt-check = "fmt --all -- --check"
```

---

## 2. Language server support: yes, with caveats

OpenCode does have an LSP client, and `rust-analyzer` is the built-in server for `.rs`. The V2 config schema still exposes `lsp` with the description "Enable built-in language servers or configure server overrides", even though the V2 documentation index does not currently have a dedicated LSP page. Treat the schema and the installed version as the contract.

Caveats:

- **LSP is currently off.** There is no project `opencode.jsonc`, the global config only contains service state, and rust-analyzer is not installed.
- **What I get from LSP is diagnostics, not navigation.** There is no `go-to-definition` or `find-references` tool in my catalog. LSP is an edit-time safety net, not a replacement for grep/read.
- **There is no direct `opencode debug lsp` command in this build (v2.2.7).** Verification is via logs and observed diagnostics rather than a dedicated diagnostics command.
- **rust-analyzer needs `cargo metadata`.** Install cargo first or the server cannot analyze the crate.
- **Do not rely on LSP alone.** `cargo clippy` is the deterministic feedback loop.

A project `opencode.jsonc` I would suggest:

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

`formatter: true` runs rustfmt after my edits. Run `cargo fmt --check` once first so the first edit does not produce a giant diff. `references` makes the local PvPoke checkout available as named context, which is useful because `PLAN.md` already points at `~/code/pvpoke` for fixture provenance.

`lsp: true` enables all built-in servers. In this repo that may also start a shell language server for the fixture `.sh` script; disable it if the extra process is noisy.

---

## 3. Beads: worth trying, but only with a rule change

Beads (`bd`) is a Dolt-powered, dependency-aware issue tracker designed for AI agents. It provides `bd ready`, claims, dependency links, comments, and durable project memory (`bd remember`).

Your repository has a deliberate documentation split, and `PLAN.md` is already serving as the task tracker. Beads would be a **second source of truth** unless you decide that:

- `PLAN.md` owns milestones, acceptance criteria, and architecture-level sequencing.
- `bd` owns granular implementation tasks, bugs, and discovered work.

If you adopt it:

- Use `bd init --skip-agents`; otherwise it rewrites `AGENTS.md`, which conflicts with your ownership rule.
- Add a short `AGENTS.md` section saying bd owns implementation tasks and `PLAN.md` owns milestones.
- Use `bd create`, `bd ready`, `bd update <id> --claim`, `bd close`, and `bd remember`; use `--json` for machine-readable output.
- Drive it through the CLI rather than an MCP server. A CLI is cheaper in context than a tool namespace, and I can call it through the shell.
- Decide the storage and git policy before init. Beads uses a Dolt database under `.beads/` and can install git hooks. `br` (a Rust port) is a lighter SQLite + JSONL alternative if Dolt feels heavy.

**Verdict:** At 5.5k lines and one crate, the overhead is probably not paid back yet. Beads becomes clearly worth it when you have multiple agents or sessions, many discovered tasks, or dependency chains across milestones. If you are already feeling task-tracking pain, try it; otherwise defer it.

---

## 4. CodeGraph: not yet

CodeGraph builds a pre-indexed code knowledge graph, stores it in local SQLite, and exposes it through a single `codegraph_explore` MCP tool. It supports Rust and OpenCode, auto-syncs on file changes, and includes impact/caller/callee queries.

Why it is not a priority here:

- The engine is about 5.5k lines across roughly 10 source files. I can read the whole crate for less context than a graph payload.
- CodeGraph's own documentation notes that its dense responses increase **residual context**; the win is on large, multi-language repositories and cross-file flow questions.
- The flow in this repo (`battle → transition → damage → data`) is easy to follow with grep and read at this size.

Revisit when you add a CLI/UI/WASM layer, a second crate, the multi-Pokémon PvPoke adapter, or when subagents repeatedly re-discover the same structure.

If you adopt it:

```sh
codegraph install --target=opencode --location=local
codegraph init
```

The installer wires the MCP server into OpenCode and adds a marker-fenced section to `AGENTS.md`; review that diff. Add `.codegraph/` to `.gitignore` if you do not want to commit the index. The MCP server is `codegraph serve --mcp`.

**Alternative:** if you want precise Rust navigation without a full graph, Serena is an LSP-backed MCP server with symbol search, references, and rename support. It is a better fit than CodeGraph for a Rust-only crate, though it is still optional.

---

## 5. Other tools, ranked

### P1: highest expected impact

| Tool | Why |
| --- | --- |
| `cargo-nextest` | Faster, clearer per-test output as the suite grows. |
| `proptest` | Property tests for determinism, replay equivalence, snapshot round-trips, energy bounds, and turn monotonicity. High value for a rules engine. |
| `cargo-mutants` | Mutation testing on rule resolution. `.gitignore` already ignores `mutants.out`, so this was anticipated. Run occasionally, not on every commit. |
| `just` (or a `Makefile`) | Encode `test`, `lint`, `fmt-check`, `fixture`, and `compat` so agents and humans use the same commands. |
| `gh` | Check CI/PR status and manage issues. Also an issue tracker if you skip Beads. |
| GitHub Actions CI | Run fmt, clippy, and tests on every push. Gives the agent a second validation signal. |

### P2 and low priority

| Tool | Why |
| --- | --- |
| `insta` | Snapshot tests for observation/result payloads. Optional because the PvPoke fixtures already form a custom golden harness. |
| `cargo-llvm-cov` | Coverage to find untested rule branches. |
| `divan` or `criterion` | Turn-resolution throughput if bulk simulation becomes a goal. |
| `cargo-fuzz` + `arbitrary` | Fuzz replay and JSON deserialization once it accepts untrusted input. |
| `ast-grep` | Structural search and rewrite for mechanical refactors. Low need at this size. |
| `cargo-semver-checks` | The `snapshot_public_api` test suggests API stability matters; run if you publish. |
| `tracing` + `tracing-subscriber` | There is no structured logging today. Turn-level traces would help debug fixture mismatches. |
| `pretty_assertions` | Readable diffs in unit tests. The crate currently has no dev-dependencies. |
| `cargo-deny` / `cargo-audit` | Supply-chain checks. Few dependencies, so low urgency. |
| `bacon` / `watchexec` | Useful for human watch loops; not needed in an agent loop. |

### CLI tools vs library crates

Some of these are Cargo subcommands or standalone binaries:

```sh
cargo install cargo-nextest cargo-mutants cargo-llvm-cov cargo-semver-checks cargo-deny cargo-audit
# just, gh, and ast-grep are also available from distro packages or their installers.
```

Others are library crates added to `Cargo.toml` (use `cargo add --dev` and let cargo choose current versions):

- `proptest`
- `insta`
- `divan` or `criterion`
- `arbitrary`
- `tracing` and `tracing-subscriber`
- `pretty_assertions`

One general principle: prefer **CLI tools** over MCP servers in this repo. Each MCP server adds a tool namespace and dispatch layer; shell commands are cheaper and easier to script.

---

## 6. What I would add to the repo

In order:

1. `rust-toolchain.toml` — pin the compiler and components.
2. `opencode.jsonc` — LSP, formatter, and the PvPoke reference.
3. `justfile` — one command surface for test, lint, format, and fixture regeneration.
4. `.opencode/skills/engine-dev/SKILL.md` — the build/test/fixture commands and key invariants. This keeps `AGENTS.md` focused on documentation ownership.
5. `.github/workflows/ci.yml` — fmt, clippy, and test on push and pull request.
6. Beads only after you decide the `PLAN.md` split; CodeGraph only when the repo grows.

Deliberately skipped for now: `bacon`, `watchexec`, heavy MCP servers, and CodeGraph. They add process or context without enough benefit at the current size.

---

## Appendix A: Tool reference

Sections 1–2 explain the config files; [Appendix B](#appendix-b-suggested-file-contents) has copy-paste versions. This appendix lists the named tools and services. Links are current as of 2026-11-11.

### Rust core

- **cargo** — Rust's build tool and package manager; compiles, tests, runs, and fetches dependencies. *Why here:* it is the minimum viable feedback loop for this crate. [GitHub](https://github.com/rust-lang/cargo) · [Docs](https://doc.rust-lang.org/cargo/)
- **rustup** — Rust toolchain installer and version manager; installs channels, components, and targets. *Why here:* pins the toolchain and adds clippy/rustfmt/rust-analyzer without mixing distro packages. [GitHub](https://github.com/rust-lang/rustup) · [Website](https://rustup.rs/)
- **clippy** — Rust linter with correctness, style, and performance checks. *Why here:* catches suspicious rule logic and common mistakes before tests do. [GitHub](https://github.com/rust-lang/rust-clippy) · [Docs](https://doc.rust-lang.org/clippy/)
- **rustfmt** — Rust source formatter. *Why here:* keeps diffs consistent and powers `formatter: true` in OpenCode. [GitHub](https://github.com/rust-lang/rustfmt) · [Docs](https://rust-lang.github.io/rustfmt/)
- **rust-analyzer** — Rust language server with diagnostics, completion, navigation, and refactors. *Why here:* powers LSP diagnostics in OpenCode and is already configured in the devcontainer. [GitHub](https://github.com/rust-lang/rust-analyzer) · [Website](https://rust-analyzer.github.io/)

### Testing and correctness

- **cargo-nextest** — Next-generation test runner for Rust; faster and clearer than `cargo test`. [GitHub](https://github.com/nextest-rs/nextest) · [Website](https://nexte.st/)
- **proptest** — Property-based testing for Rust; generates inputs and shrinks failures. *Why here:* ideal for determinism, replay equivalence, snapshot round-trips, energy bounds, and turn invariants. [GitHub](https://github.com/proptest-rs/proptest) · [Website](https://proptest-rs.github.io/proptest/)
- **cargo-mutants** — Mutation testing: mutates code and checks whether tests catch it. *Why here:* validates that rule-resolution tests actually constrain behavior; `.gitignore` already anticipates `mutants.out`. [GitHub](https://github.com/sourcefrog/cargo-mutants) · [Website](https://mutants.rs/)
- **insta** — Snapshot testing for Rust with inline and file snapshots and a review workflow. *Why here:* optional for observation/result payload snapshots; the PvPoke fixtures already cover cross-engine compatibility. [GitHub](https://github.com/mitsuhiko/insta) · [Website](https://insta.rs/)
- **cargo-llvm-cov** — Code coverage using LLVM source-based coverage. [GitHub](https://github.com/taiki-e/cargo-llvm-cov) · [Docs](https://docs.rs/cargo-llvm-cov)
- **divan** — Lightweight Rust benchmarking library with a simple API and fast runs. *Why here:* turn-resolution throughput if bulk simulation becomes a goal. [GitHub](https://github.com/nvzqz/divan) · [Docs](https://docs.rs/divan)
- **criterion** — Statistics-driven Rust benchmarking with regression detection and plots. *Why here:* heavier alternative to divan for performance work. [GitHub](https://github.com/bheisler/criterion.rs) · [Book](https://bheisler.github.io/criterion.rs/book/)
- **cargo-fuzz** — Coverage-guided fuzzing for Rust. *Why here:* useful once replay or fixture JSON parsing accepts untrusted input. [GitHub](https://github.com/rust-fuzz/cargo-fuzz) · [Book](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- **arbitrary** — Trait for deriving structured fuzz inputs; used with cargo-fuzz. [GitHub](https://github.com/rust-fuzz/arbitrary) · [Docs](https://docs.rs/arbitrary)
- **pretty_assertions** — Drop-in `assert_eq!` replacement with readable diffs. *Why here:* improves unit-test failure output; the crate currently has no dev-dependencies. [GitHub](https://github.com/rust-pretty-assertions/rust-pretty-assertions) · [Docs](https://docs.rs/pretty_assertions)
- **cargo-semver-checks** — Lints Rust API changes for semantic-versioning breakage. *Why here:* the `snapshot_public_api` test suggests public API stability matters; run if you publish. No dedicated website; the Cargo SemVer reference explains the rules. [GitHub](https://github.com/obi1kenobi/cargo-semver-checks) · [Cargo SemVer reference](https://doc.rust-lang.org/cargo/reference/semver.html)

### Workflow and project hygiene

- **just** — Command runner with a simple `justfile` syntax. *Why here:* encode test/lint/fmt/fixture commands once. [GitHub](https://github.com/casey/just) · [Website](https://just.systems/)
- **GNU Make** — Classic build and automation tool; available on most systems. *Why here:* zero-install alternative to just for a small command menu. [Savannah](https://savannah.gnu.org/projects/make) · [Website](https://www.gnu.org/software/make/)
- **gh** — GitHub's official CLI for issues, pull requests, releases, and Actions. *Why here:* check CI/PR status; use GitHub Issues if you skip Beads. [GitHub](https://github.com/cli/cli) · [Website](https://cli.github.com/)
- **GitHub Actions** — GitHub's CI/CD service. *Why here:* run fmt, clippy, and tests on every push and give the agent a second validation signal. [Product](https://github.com/features/actions) · [Docs](https://docs.github.com/actions)
- **cargo-deny** — Cargo dependency linting for licenses, bans, advisories, and sources. [GitHub](https://github.com/EmbarkStudios/cargo-deny) · [Docs](https://embarkstudios.github.io/cargo-deny/)
- **cargo-audit** — Audits `Cargo.lock` against the RustSec advisory database. [GitHub](https://github.com/rustsec/rustsec) · [Website](https://rustsec.org/)
- **bacon** — Background Rust code checker that runs cargo check/clippy/test and shows results in a TUI. *Why here:* nice for humans, not needed in an agent loop. [GitHub](https://github.com/Canop/bacon) · [Website](https://dystroy.org/bacon/)
- **watchexec** — Runs commands when files change. *Why here:* general-purpose file watcher; not needed in an agent loop. [GitHub](https://github.com/watchexec/watchexec) · [Website](https://watchexec.github.io/)

### Code intelligence

- **OpenCode** — The agent harness running this session; supports configuration, LSP, formatters, MCP, skills, and references. *Why here:* the project config in section 2 is the highest-leverage integration point. [GitHub](https://github.com/sst/opencode) · [Website](https://opencode.ai/)
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
- **dnf** — Fedora's package manager. *Why here:* alternative install channel for cargo, clippy, rustfmt, and rust-analyzer. [GitHub](https://github.com/rpm-software-management/dnf5) · [Docs](https://docs.fedoraproject.org/)

---

## Appendix B: Suggested file contents

These are starting points, not requirements. Adjust versions and paths to your environment.

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
test:
    cargo test --all-targets

lint:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check: fmt-check lint test

fixture file:
    PVPOKE_DIR=$HOME/code/pvpoke sh engine/tools/pvpoke/generate-fixture.sh {{file}}
```

### `.cargo/config.toml` (alternative to just)

```toml
[alias]
t = "test --all-targets"
lint = "clippy --all-targets -- -D warnings"
fmt-check = "fmt --all -- --check"
```

### `.github/workflows/ci.yml`

```yaml
name: ci
on:
  push:
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install 1.99 --component clippy --component rustfmt
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all-targets
```
