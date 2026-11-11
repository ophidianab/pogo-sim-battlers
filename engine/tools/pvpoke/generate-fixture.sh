#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    printf 'Usage: sh engine/tools/pvpoke/generate-fixture.sh <fixture.json> [output.json]\n' >&2
    exit 2
fi

repo_root=$(git rev-parse --show-toplevel)
pvpoke_root=${PVPOKE_DIR:-"$HOME/code/pvpoke"}
container_engine=${CONTAINER_ENGINE:-}
node_image=${NODE_IMAGE:-docker.io/library/node:20-alpine}

if [ -z "$container_engine" ]; then
    if command -v podman >/dev/null 2>&1; then
        container_engine=podman
    else
        container_engine=docker
    fi
fi

fixture_path=$(realpath "$1")
output_path=$(realpath -m "${2:-$1}")
case "$fixture_path" in
    "$repo_root"/*) fixture_relative=${fixture_path#"$repo_root"/} ;;
    *) printf 'Fixture must be inside the simulator repository.\n' >&2; exit 2 ;;
esac
case "$output_path" in
    "$repo_root"/*) output_relative=${output_path#"$repo_root"/} ;;
    *) printf 'Output must be inside the simulator repository.\n' >&2; exit 2 ;;
esac

if [ ! -d "$pvpoke_root/.git" ] || [ ! -f "$pvpoke_root/src/data/gamemaster.json" ]; then
    printf 'PVPOKE_DIR must point to a PvPoke checkout containing src/data/gamemaster.json.\n' >&2
    exit 2
fi
if [ -n "$(git -C "$pvpoke_root" status --porcelain -- src)" ]; then
    printf 'PvPoke src/ has local changes; refusing to generate fixtures from an unpinned tree.\n' >&2
    exit 2
fi

pvpoke_commit=$(git -C "$pvpoke_root" rev-parse HEAD)
gamemaster_blob=$(git -C "$pvpoke_root" ls-tree HEAD src/data/gamemaster.json | awk '{ print $3 }')
if [ -z "$gamemaster_blob" ]; then
    printf 'Could not identify the tracked Game Master blob.\n' >&2
    exit 2
fi

container_args=""
if [ "$container_engine" = "podman" ]; then
    "$container_engine" run --rm --network=none --security-opt label=disable \
        --userns=keep-id --user "$(id -u):$(id -g)" \
        -v "$repo_root:/workspace:rw" -v "$pvpoke_root:/pvpoke:ro" \
        -w /workspace -e PVPOKE_COMMIT="$pvpoke_commit" \
        -e PVPOKE_GAMEMASTER_BLOB="$gamemaster_blob" \
        "$node_image" node /workspace/engine/tools/pvpoke/generate-fixture.mjs \
        "/workspace/$fixture_relative" "/workspace/$output_relative"
else
    "$container_engine" run --rm --network=none --user "$(id -u):$(id -g)" \
        -v "$repo_root:/workspace:rw" -v "$pvpoke_root:/pvpoke:ro" \
        -w /workspace -e PVPOKE_COMMIT="$pvpoke_commit" \
        -e PVPOKE_GAMEMASTER_BLOB="$gamemaster_blob" \
        "$node_image" node /workspace/engine/tools/pvpoke/generate-fixture.mjs \
        "/workspace/$fixture_relative" "/workspace/$output_relative"
fi