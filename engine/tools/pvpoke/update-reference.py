#!/usr/bin/env python3
"""Update the pinned PvPoke reference used for compatibility fixtures.

This automates the steps in UPDATE.md:

1. Optionally move the local PvPoke checkout to a new ref.
2. Record the new commit and gamemaster blob in every committed fixture.
3. Update pinned documentation and any other tracked reference to the old pin.
4. Refresh the Rust engine's embedded Game Master data.
5. Regenerate every committed fixture.
6. Verify compatibility and that regeneration is byte-identical.

The script never commits. Review the resulting diff before committing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

GENERATOR = Path("engine/tools/pvpoke/generate-fixture.sh")
FIXTURE_DIR = Path("engine/tests/fixtures/pvpoke")
DATA_DIR = Path("engine/data")


class UpdateError(Exception):
    pass


def fail(message: str) -> None:
    raise UpdateError(message)


def info(message: str = "") -> None:
    print(message, flush=True)


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    capture: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )
    if check and result.returncode != 0:
        if capture:
            if result.stdout:
                print(result.stdout, end="", file=sys.stderr)
            if result.stderr:
                print(result.stderr, end="", file=sys.stderr)
        fail(f"command failed ({result.returncode}): {' '.join(command)}")
    return result


def git(pvpoke_dir: Path, *args: str) -> str:
    return run(["git", "-C", str(pvpoke_dir), *args], capture=True).stdout.strip()


def repo_root() -> Path:
    result = run(
        [
            "git",
            "-C",
            str(Path(__file__).resolve().parent),
            "rev-parse",
            "--show-toplevel",
        ],
        capture=True,
    )
    return Path(result.stdout.strip())


# ---------------------------------------------------------------------------
# JSON formatting
# ---------------------------------------------------------------------------


def format_value(value: Any, unit: int, level: int = 0) -> str:
    """Pretty-print JSON in the existing engine-data style.

    Objects are always expanded. Arrays of scalars stay on one line, matching
    the hand-maintained style of engine/data/*.json.
    """

    if isinstance(value, dict):
        if not value:
            return "{}"
        pad = " " * (unit * (level + 1))
        close = " " * (unit * level)
        items = [
            f"{pad}{json.dumps(key, ensure_ascii=False)}: {format_value(item, unit, level + 1)}"
            for key, item in value.items()
        ]
        return "{\n" + ",\n".join(items) + "\n" + close + "}"
    if isinstance(value, list):
        if not value:
            return "[]"
        if all(not isinstance(item, (dict, list)) for item in value):
            return (
                "["
                + ", ".join(json.dumps(item, ensure_ascii=False) for item in value)
                + "]"
            )
        pad = " " * (unit * (level + 1))
        close = " " * (unit * level)
        items = [f"{pad}{format_value(item, unit, level + 1)}" for item in value]
        return "[\n" + ",\n".join(items) + "\n" + close + "]"
    return json.dumps(value, ensure_ascii=False)


def write_if_changed(path: Path, content: str, *, write: bool) -> bool:
    current = path.read_text() if path.exists() else None
    if current == content:
        info(f"  unchanged {path.relative_to(path.parents[1])}")
        return False
    if not write:
        info(f"  would update {path.relative_to(path.parents[1])}")
        return True
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(content)
    temporary.replace(path)
    info(f"  updated {path.relative_to(path.parents[1])}")
    return True


# ---------------------------------------------------------------------------
# Embedded data refresh
# ---------------------------------------------------------------------------


def extract_cpms(pvpoke_dir: Path) -> list[str]:
    source = (pvpoke_dir / "src/js/pokemon/Pokemon.js").read_text()
    match = re.search(r"var cpms = \[(.*?)\];", source, re.S)
    if not match:
        fail("could not find the CPM table in src/js/pokemon/Pokemon.js")
    tokens = [token.strip() for token in match.group(1).split(",") if token.strip()]
    for token in tokens:
        float(token)
    if len(tokens) < 2:
        fail("CPM table is unexpectedly short")
    return tokens


def extract_type_chart(pvpoke_dir: Path) -> dict[str, dict[str, list[str]]]:
    source = (pvpoke_dir / "src/js/battle/DamageCalculator.js").read_text()
    pattern = re.compile(
        r'case "(\w+)":\s*'
        r"traits = \{ resistances: (\[[^\]]*\]),\s*"
        r"weaknesses: (\[[^\]]*\]),\s*"
        r"immunities: (\[[^\]]*\]) \};",
        re.S,
    )
    chart: dict[str, dict[str, list[str]]] = {}
    for match in pattern.finditer(source):
        move_type, resistances, weaknesses, immunities = match.groups()
        chart[move_type] = {
            "resistances": json.loads(resistances),
            "weaknesses": json.loads(weaknesses),
            "immunities": json.loads(immunities),
        }
    if len(chart) != 18:
        fail(f"expected 18 type chart entries, found {len(chart)}")
    return chart


def dump_cpm(tokens: list[str]) -> str:
    lines = [
        "{",
        '  "startingLevel": 1.0,',
        '  "levelStep": 0.5,',
        '  "values": [',
    ]
    for index in range(0, len(tokens), 10):
        chunk = tokens[index : index + 10]
        line = "    " + ", ".join(chunk)
        if index + 10 < len(tokens):
            line += ","
        lines.append(line)
    lines.extend(["  ]", "}"])
    return "\n".join(lines) + "\n"


def load_json(path: Path) -> Any:
    return json.loads(path.read_text()) if path.exists() else None


def describe_delta(
    label: str, old: list[dict[str, Any]] | None, new: list[dict[str, Any]], key: str
) -> None:
    old = old or []
    old_by_id = {item[key]: item for item in old}
    new_by_id = {item[key]: item for item in new}
    added = [identifier for identifier in new_by_id if identifier not in old_by_id]
    removed = [identifier for identifier in old_by_id if identifier not in new_by_id]
    changed = [
        identifier
        for identifier in new_by_id
        if identifier in old_by_id and old_by_id[identifier] != new_by_id[identifier]
    ]
    info(
        f"  {label}: {len(new)} entries (+{len(added)}, -{len(removed)}, ~{len(changed)})"
    )
    if added:
        info(f"    added: {', '.join(added)}")
    if removed:
        info(f"    removed: {', '.join(removed)}")
    if changed:
        preview = ", ".join(changed[:10])
        suffix = "" if len(changed) <= 10 else f", ... ({len(changed) - 10} more)"
        info(f"    changed: {preview}{suffix}")

    old_fields = {field for item in old for field in item}
    new_fields = {field for item in new for field in item}
    added_fields = sorted(new_fields - old_fields)
    if added_fields:
        info(f"    new fields: {', '.join(added_fields)}")


def describe_cpm_delta(old: Any, new_tokens: list[str]) -> None:
    old_values = old.get("values", []) if isinstance(old, dict) else []
    new_values = [float(token) for token in new_tokens]
    changed = [
        (index, old_values[index], new_values[index])
        for index in range(min(len(old_values), len(new_values)))
        if old_values[index] != new_values[index]
    ]
    info(
        f"  cpm.json: {len(new_values)} values (+{max(0, len(new_values) - len(old_values))}, -{max(0, len(old_values) - len(new_values))}, ~{len(changed)})"
    )
    for index, previous, current in changed:
        level = 1.0 + index * 0.5
        info(f"    level {level:g}: {previous} -> {current}")


def describe_type_chart_delta(old: Any, new: dict[str, dict[str, list[str]]]) -> None:
    old = old if isinstance(old, dict) else {}
    changed = [move_type for move_type in new if old.get(move_type) != new[move_type]]
    info(f"  type_chart.json: {len(new)} types (~{len(changed)})")
    if changed:
        info(f"    changed: {', '.join(changed)}")


def refresh_embedded_data(pvpoke_dir: Path, data_dir: Path, *, write: bool) -> bool:
    info("Refreshing embedded engine data")
    gamemaster = json.loads((pvpoke_dir / "src/data/gamemaster.json").read_text())
    pokemon = gamemaster.get("pokemon")
    moves = gamemaster.get("moves")
    if not isinstance(pokemon, list) or not isinstance(moves, list):
        fail("gamemaster.json must contain pokemon and moves arrays")

    cpm_tokens = extract_cpms(pvpoke_dir)
    type_chart = extract_type_chart(pvpoke_dir)

    old_pokemon = load_json(data_dir / "pokemon.json")
    old_moves = load_json(data_dir / "moves.json")
    old_cpm = load_json(data_dir / "cpm.json")
    old_type_chart = load_json(data_dir / "type_chart.json")

    describe_delta("pokemon.json", old_pokemon, pokemon, "speciesId")
    describe_delta("moves.json", old_moves, moves, "moveId")
    describe_cpm_delta(old_cpm, cpm_tokens)
    describe_type_chart_delta(old_type_chart, type_chart)

    changed = False
    changed |= write_if_changed(
        data_dir / "pokemon.json", format_value(pokemon, 4), write=write
    )
    changed |= write_if_changed(
        data_dir / "moves.json", format_value(moves, 4), write=write
    )
    changed |= write_if_changed(
        data_dir / "cpm.json", dump_cpm(cpm_tokens), write=write
    )
    changed |= write_if_changed(
        data_dir / "type_chart.json", format_value(type_chart, 2) + "\n", write=write
    )
    return changed


# ---------------------------------------------------------------------------
# Fixture provenance and documentation
# ---------------------------------------------------------------------------


def read_fixture_pins(fixture_dir: Path) -> tuple[str, str, int]:
    files = sorted(fixture_dir.glob("*.json"))
    if not files:
        fail(f"no fixtures found in {fixture_dir}")
    pins: set[tuple[str, str]] = set()
    for path in files:
        fixture = json.loads(path.read_text())
        reference = fixture.get("reference")
        if not isinstance(reference, dict):
            fail(f"{path.name}: missing reference object")
        commit = reference.get("commit")
        blob = reference.get("gamemasterBlob")
        if not isinstance(commit, str) or not isinstance(blob, str):
            fail(f"{path.name}: missing reference.commit or reference.gamemasterBlob")
        pins.add((commit, blob))
    if len(pins) != 1:
        details = "\n".join(
            f"  {path.name}: {json.loads(path.read_text()).get('reference', {}).get('commit')}"
            for path in files
        )
        fail(f"committed fixtures do not share one reference pin:\n{details}")
    commit, blob = next(iter(pins))
    return commit, blob, len(files)


def update_fixture_pins(
    fixture_dir: Path, commit: str, blob: str, *, write: bool
) -> int:
    changed = 0
    for path in sorted(fixture_dir.glob("*.json")):
        fixture = json.loads(path.read_text())
        reference = fixture["reference"]
        if (
            reference.get("commit") == commit
            and reference.get("gamemasterBlob") == blob
        ):
            continue
        reference["commit"] = commit
        reference["gamemasterBlob"] = blob
        changed += 1
        if write:
            temporary = path.with_name(path.name + ".tmp")
            temporary.write_text(json.dumps(fixture, indent=2) + "\n")
            temporary.replace(path)
    info(
        f"Fixture provenance: {changed} file(s) {'would change' if not write else 'updated'}"
    )
    return changed


def update_documentation(
    root: Path,
    old_commit: str,
    old_blob: str,
    new_commit: str,
    new_blob: str,
    new_date: str,
    *,
    write: bool,
) -> list[Path]:
    matches = run(
        [
            "git",
            "-C",
            str(root),
            "grep",
            "-l",
            "-I",
            "-F",
            "-e",
            old_commit,
            "-e",
            old_blob,
            "--",
            ".",
        ],
        capture=True,
        check=False,
    )
    if matches.returncode not in (0, 1):
        fail("git grep failed while looking for pinned reference hashes")

    fixture_prefix = str(FIXTURE_DIR) + "/"
    candidates = [
        Path(line)
        for line in matches.stdout.splitlines()
        if line and not line.startswith(fixture_prefix)
    ]
    changed: list[Path] = []
    date_pattern = re.compile(
        r"(`" + re.escape(new_commit) + r"`\s*)\(\d{4}-\d{2}-\d{2}\)"
    )
    for relative in sorted(candidates):
        path = root / relative
        text = path.read_text()
        updated = text.replace(old_commit, new_commit).replace(old_blob, new_blob)
        updated = date_pattern.sub(
            lambda match: f"{match.group(1)}({new_date})", updated
        )
        if updated == text:
            continue
        changed.append(relative)
        info(f"  update {relative}")
        if write:
            temporary = path.with_name(path.name + ".tmp")
            temporary.write_text(updated)
            temporary.replace(path)

    if not candidates:
        info("Pinned documentation: no references found outside fixtures")

    if old_commit == new_commit and old_blob == new_blob:
        return changed

    remaining = run(
        [
            "git",
            "-C",
            str(root),
            "grep",
            "-l",
            "-I",
            "-F",
            "-e",
            old_commit,
            "-e",
            old_blob,
            "--",
            ".",
        ],
        capture=True,
        check=False,
    )
    lingering = [
        line
        for line in remaining.stdout.splitlines()
        if line and not line.startswith(str(FIXTURE_DIR) + "/")
    ]
    if lingering:
        info("  warning: old reference still appears in:")
        for line in lingering:
            info(f"    {line}")

    return changed


# ---------------------------------------------------------------------------
# Fixture regeneration and verification
# ---------------------------------------------------------------------------


def fixture_hashes(fixture_dir: Path) -> dict[str, str]:
    return {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(fixture_dir.glob("*.json"))
    }


def regenerate_fixtures(
    root: Path, pvpoke_dir: Path, fixture_dir: Path, *, quiet: bool
) -> None:
    environment = os.environ.copy()
    environment["PVPOKE_DIR"] = str(pvpoke_dir)
    script = root / GENERATOR
    files = sorted(fixture_dir.glob("*.json"))
    for index, path in enumerate(files, start=1):
        if not quiet:
            info(f"  [{index}/{len(files)}] {path.name}")
        result = run(
            ["sh", str(script), str(path)],
            cwd=root,
            env=environment,
            capture=quiet,
            check=False,
        )
        if result.returncode != 0:
            if quiet:
                if result.stdout:
                    print(result.stdout, end="", file=sys.stderr)
                if result.stderr:
                    print(result.stderr, end="", file=sys.stderr)
            fail(f"failed to regenerate {path.name}")


def verify_fixture_pins(fixture_dir: Path, commit: str, blob: str) -> None:
    for path in sorted(fixture_dir.glob("*.json")):
        fixture = json.loads(path.read_text())
        reference = fixture["reference"]
        if reference.get("commit") != commit or reference.get("gamemasterBlob") != blob:
            fail(f"{path.name}: provenance was not updated to the new pin")
        if not reference.get("generatorRevision"):
            fail(f"{path.name}: missing generatorRevision")


def run_checks(root: Path) -> None:
    if shutil.which("just") is None:
        fail("just is required to run the verification recipes")
    info("Running just compat")
    run(["just", "compat"], cwd=root)
    info("Running just check")
    run(["just", "check"], cwd=root)


def changed_recipe_count(root: Path) -> None:
    result = run(
        [
            "git",
            "-C",
            str(root),
            "diff",
            "--stat",
            "--",
            "engine/data",
            str(FIXTURE_DIR),
            "PLAN.md",
        ],
        capture=True,
        check=False,
    )
    if result.stdout.strip():
        info(result.stdout.strip())


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Update the pinned PvPoke reference, fixtures, and embedded data.",
    )
    parser.add_argument(
        "--pvpoke-dir",
        default=os.environ.get("PVPOKE_DIR", str(Path.home() / "code/pvpoke")),
        help="PvPoke checkout (default: $PVPOKE_DIR or ~/code/pvpoke)",
    )
    update = parser.add_mutually_exclusive_group()
    update.add_argument(
        "--ref",
        help="Fetch and check out this git ref in the PvPoke checkout before updating",
    )
    update.add_argument(
        "--pull",
        action="store_true",
        help="Fast-forward the current PvPoke branch before updating",
    )
    parser.add_argument(
        "--skip-data", action="store_true", help="Do not refresh engine/data"
    )
    parser.add_argument(
        "--skip-fixtures", action="store_true", help="Do not regenerate fixtures"
    )
    parser.add_argument(
        "--skip-checks",
        action="store_true",
        help="Skip just compat/check and the byte-identical regeneration check",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Report planned changes without writing files or running regeneration/checks",
    )
    parser.add_argument(
        "--yes", action="store_true", help="Do not ask for confirmation"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = repo_root()
    pvpoke_dir = Path(args.pvpoke_dir).expanduser().resolve()
    fixture_dir = root / FIXTURE_DIR
    data_dir = root / DATA_DIR

    if (
        not (pvpoke_dir / ".git").is_dir()
        or not (pvpoke_dir / "src/data/gamemaster.json").is_file()
    ):
        fail(f"{pvpoke_dir} is not a PvPoke checkout with src/data/gamemaster.json")

    info(f"Repository: {root}")
    info(f"PvPoke checkout: {pvpoke_dir}")

    if not args.dry_run:
        if args.ref:
            info(f"Fetching and checking out {args.ref}")
            run(["git", "-C", str(pvpoke_dir), "fetch", "--all", "--tags", "--prune"])
            run(["git", "-C", str(pvpoke_dir), "checkout", "--detach", args.ref])
        elif args.pull:
            info("Fast-forwarding the PvPoke checkout")
            run(["git", "-C", str(pvpoke_dir), "pull", "--ff-only"])

    if git(pvpoke_dir, "status", "--porcelain", "--", "src"):
        fail("PvPoke src/ has local changes; refusing to update from an unpinned tree")

    new_commit = git(pvpoke_dir, "rev-parse", "HEAD")
    new_blob = git(pvpoke_dir, "ls-tree", "HEAD", "src/data/gamemaster.json").split()[2]
    hashed_blob = git(pvpoke_dir, "hash-object", "src/data/gamemaster.json")
    if new_blob != hashed_blob:
        fail(
            "tracked gamemaster blob does not match the working file; "
            f"ls-tree={new_blob}, hash-object={hashed_blob}"
        )
    new_date = git(pvpoke_dir, "show", "-s", "--format=%cs", new_commit)

    old_commit, old_blob, fixture_count = read_fixture_pins(fixture_dir)
    try:
        old_date = git(pvpoke_dir, "show", "-s", "--format=%cs", old_commit)
    except UpdateError:
        old_date = "unknown"

    info("")
    info(f"Previous pin: {old_commit} ({old_date}), blob {old_blob}")
    info(f"New pin:      {new_commit} ({new_date}), blob {new_blob}")
    info(f"Fixtures:     {fixture_count}")
    if old_commit == new_commit and old_blob == new_blob:
        info("Reference pin is unchanged; this run will only refresh derived data.")

    if not args.dry_run and not args.yes:
        if not sys.stdin.isatty():
            fail("refusing to modify the repository non-interactively; pass --yes")
        answer = input("Proceed? [y/N] ").strip().lower()
        if answer not in {"y", "yes"}:
            info("Aborted.")
            return 1

    info("")
    update_fixture_pins(fixture_dir, new_commit, new_blob, write=not args.dry_run)

    info("")
    info("Updating pinned documentation")
    update_documentation(
        root,
        old_commit,
        old_blob,
        new_commit,
        new_blob,
        new_date,
        write=not args.dry_run,
    )

    if not args.skip_data:
        info("")
        refresh_embedded_data(pvpoke_dir, data_dir, write=not args.dry_run)

    if args.dry_run:
        info("")
        info("Dry run complete; no files were modified.")
        return 0

    if not args.skip_fixtures:
        info("")
        info("Regenerating fixtures")
        regenerate_fixtures(root, pvpoke_dir, fixture_dir, quiet=False)
        verify_fixture_pins(fixture_dir, new_commit, new_blob)

        if not args.skip_checks:
            info("")
            info("Checking that a second regeneration is byte-identical")
            before = fixture_hashes(fixture_dir)
            regenerate_fixtures(root, pvpoke_dir, fixture_dir, quiet=True)
            after = fixture_hashes(fixture_dir)
            if before != after:
                changed = sorted(name for name in before if before[name] != after[name])
                fail(f"fixture regeneration is not idempotent: {', '.join(changed)}")

    if not args.skip_checks:
        info("")
        run_checks(root)

    info("")
    info("Change summary")
    changed_recipe_count(root)
    info("Review the full diff with: git diff")
    info("Done.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except UpdateError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
