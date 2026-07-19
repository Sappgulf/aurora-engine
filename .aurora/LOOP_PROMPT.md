# Aurora Lumen Foundry Loop

You are the Aurora Lumen Foundry agent working inside the aurora-engine repository.

Perform exactly one coherent task, validate it completely, persist evidence and
learning, then exit.

## Read first

1. `.aurora/CONSTITUTION.md`
2. `.aurora/PRODUCT_NORTH_STAR.md`
3. `.aurora/ENGINE_GAME_BOUNDARY.md`
4. `.aurora/CURRENT_EPIC.md`
5. `.aurora/BACKLOG.yaml`
6. `.aurora/LEARNINGS.md`
7. `.aurora/PERF_BUDGETS.toml`
8. `.aurora/PLAYTEST_MATRIX.toml`
9. `git log -5`
10. `reports/latest.json` when present

## Iterate

Choose the highest-priority unblocked backlog task that fits one coherent diff.
Write measurable acceptance criteria before implementation. Keep engine and
game ownership aligned with `ENGINE_GAME_BOUNDARY.md`.

Run every relevant gate: formatting, Clippy with all features, workspace tests,
WASM check, actual Trunk release build, MCP protocol tests, deterministic trace
twice, and native/browser screenshot review for user-facing work.

Write evidence to `reports/latest.json`, update the backlog and learnings, and
commit only after the gates pass. Never emit `AURORA_EPIC_COMPLETE` while a
current-epic acceptance item or known P0/P1 regression remains.

