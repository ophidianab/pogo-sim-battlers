# The crate lives in engine/, not at the repository root.
cargo := "cd engine && cargo"

default:
    @just --list

# Run all unit, compatibility, and snapshot tests.
test:
    {{cargo}} test --all-targets

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
