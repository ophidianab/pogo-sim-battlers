Updating the pinned PvPoke reference touches both provenance and golden data:

1. **Update the reference checkout**
   - Pull `/home/aaron/code/pvpoke` to the new commit.
   - Record the new commit hash and the new `src/data/gamemaster.json` blob hash (`git hash-object src/data/gamemaster.json`).

2. **Update fixture provenance**
   - Change `reference.commit` and `reference.gamemasterBlob` in every file under `engine/tests/fixtures/pvpoke/`.
   - Leave `generatorRevision` alone unless the harness itself changes; that tracks `generate-fixture.mjs`, not the reference data.

3. **Update pinned documentation**
   - Update the commit/blob recorded in `PLAN.md` (the manual prerequisite line) and anywhere else the reference is pinned.
   - Ensure `generate-fixture.sh` / the fixture recipe exports the new commit so the generator’s checks pass.

4. **Update the engine’s embedded data**
   - Regenerate or refresh the Rust engine’s embedded Game Master from the new PvPoke data so `DataCatalog::embedded()` matches the fixtures.
   - Check whether new species/moves/buff fields need parsing or test coverage.

5. **Regenerate every fixture**
   - Run `just fixture` for each fixture.
   - Expect `expectedResult` diffs: stats, damage, ratings, timelines, and possibly battle length will change. Review them for plausibility rather than blindly accepting goldens.

6. **Investigate behavioral changes**
   - If outputs differ beyond simple stat changes, decide whether it’s an intended Game Master update or an engine incompatibility.
   - Update documented compatibility deltas if reference behavior changed.

7. **Verify**
   - Run `just compat` and `just check`.
   - Confirm fixture regeneration is still byte-identical on a second pass.

In short: new commit + blob in every fixture and the docs, refresh the embedded catalog, regenerate and review goldens, then run the full check.
