# Klipo development workflow

Implement the authorized issue in this task's Git worktree. Read AGENTS.md and the relevant source/tests before editing. Check completed subtasks and existing behavior first; do not create churn when the requested behavior already exists.

## Implementation

Keep changes focused and follow the repository's Rust, React and TypeScript conventions. Preserve platform guards for macOS behavior. Add regression tests for defects and meaningful coverage for new behavior. Do not weaken checks, change unrelated dependency versions or reformat unrelated files to make validation pass.

The factory owns commits, pushes, PR publication and Linear updates. Do not perform those actions from the coding session. Do not print credentials or access unrelated host services. Reviews, risk classification and automatic merging are outside this development workflow.

## Validation

Use the installed tools on the worker host and run checks after the final source edit. The factory also runs its configured independent validator after the coding process stops.

Frontend checks:

```sh
bun install --frozen-lockfile
bun run --no-install biome check
bun run build
bun run test
```

Rust checks:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --features single-instance-test
```

Use the platform's required desktop session for integration tests; Linux desktop tests need an appropriate display and D-Bus session. Native macOS clipboard/focus behavior and x86_64 release packaging require their corresponding environments. Report missing prerequisites, baseline failures and unverified acceptance criteria accurately.

## Handoff

Summarize the entire branch diff against FETCH_HEAD, including retained changes from previous attempts, and report final validation results. Follow .github/pull_request_template.md when it exists; mark only verified checks as passed. Explain no-change outcomes and blockers. Do not claim that a PR was merged or an issue completed: publication happens after the coding session.
