# Klipo

Klipo is a macOS clipboard manager built with Tauri, React, TypeScript, and Rust. It stores recent text and image clipboard entries and provides a floating picker for selecting and pasting them.

## Features

- Text and image clipboard history
- Persistent history stored locally
- Global shortcut to open the picker
- Configurable picker shortcuts
- Menu bar resident mode
- Rotating JSON diagnostic logs

Klipo currently supports macOS only. Linux and Windows support are not implemented.

## Install

1. Download the latest `.dmg` from the [GitHub Releases page](https://github.com/gustavosvalentim/klipo/releases).
2. Move Klipo to the Applications folder.
3. Launch Klipo.
4. Enable Klipo under **System Settings > Privacy & Security > Accessibility**.
5. Restart Klipo.

Accessibility permission is required to paste into the previously active application.

## Usage

- `Cmd+Shift+V`: open the picker
- `Up` / `Down`: navigate entries
- `Enter`: paste the selected entry
- `Delete`: remove the selected entry
- `Esc`: close the picker

Open **Settings** from the menu bar to change the picker shortcuts. Only the shortcut for opening Klipo is global.

## Development

Requirements:

- macOS
- Rust and Cargo
- Bun
- Tauri development dependencies for macOS

```sh
bun install
bun run tauri dev
```

Build the application:

```sh
bun run tauri build
```

Build only the frontend:

```sh
bun run build
```

Validate the frontend before opening a pull request:

```sh
bun install --frozen-lockfile
bun run --no-install biome check
bun run build
```

There is currently no frontend test command or Vitest test suite configured, so the frontend CI runs the checks above. Add a test command and test suite before enabling frontend tests in CI.

Format the project:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml
bun run format
```
