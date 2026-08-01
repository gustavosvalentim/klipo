# Repository Guidelines

## About the project

Klipo is a multi-platform clipboard manager for macOS.

## Stack

- Rust
- Tauri
- React
- TypeScript
- Vite
- Tailwind
- Vitest

## Directory structure

- `src-tauri/`: contains Rust code and configuration for the Tauri app.
- - `src-tauri/Cargo.toml`: Rust package manifest.
- - `src-tauri/tauri.conf.json`: Tauri configuration.
- - `src-tauri/src/capabilities/`: contains Tauri permissions.
- - `src-tauri/src/`: contains the business logic and infrastructure wiring.
- `src/`: contains TypeScript code for the React frontend.
- - `src/App.tsx`: the root component.
- - `src/main.tsx`: the entry point for the React app.
- - `src/vite-env.d.ts`: TypeScript declarations for the Vite environment.
- - `src/components/`: contains React components.

## Build, Test, and Development Commands

- `bun install` installs frontend and Tauri CLI dependencies.
- `bun run tauri dev` starts the complete desktop application for local development.
- `bun run dev` runs only the Vite frontend server.
- `bun run build` type-checks TypeScript and builds frontend assets.
- `bun run tauri build` produces release bundles in `src-tauri/target/release/bundle/`.
- `bun run format` formats and checks frontend files with Biome.
- `cargo fmt --manifest-path src-tauri/Cargo.toml` formats the Rust backend.
- `cargo test --manifest-path src-tauri/Cargo.toml` runs Rust tests when present.

## Coding Style & Naming Conventions

### Frontend

- Use TypeScript and React function components.
- Follow Biome: tabs for indentation, double quotes in JavaScript/TypeScript, and organized imports.
- Name components in PascalCase (for example, `ListItem.tsx`); use camelCase for functions, variables, and TypeScript props.

### Rust

- Follow `rustfmt` for Rust; use `snake_case` for functions/modules and `PascalCase` for types.
- Keep macOS-specific behavior behind appropriate target checks and avoid broad platform assumptions in shared code.
- Avoid early returns. Prefer explicit control flows. Use early returns only on prechecks.

## Testing Guidelines

- When fixing a bug, add a test case that reproduces the bug and fails without a fix.
- When adding features, add tests that verify the behavior of the feature.
- Use Vitest for testing React components.
- Use `cargo test` for testing Rust code.

## Commit & Pull Request Guidelines

- Use the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) format for commit messages.
- Branch names should be prefixed with the type of change (e.g., `feat/` for new features, `fix/` for bug fixes, etc.).
- If there is a ticket number associated with the pull request, include it in the name of the branch, e.g. `feat/TICKET-123-<change-title>`
