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
