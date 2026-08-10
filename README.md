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

> [!WARNING]
> Klipo's macOS DMG is unsigned and not notarized. Download it only from the official [GitHub Releases page](https://github.com/gustavosvalentim/klipo/releases).

1. Download the latest `.dmg` from the [GitHub Releases page](https://github.com/gustavosvalentim/klipo/releases).
2. Move Klipo to the Applications folder.
3. To open Klipo the first time, Control-click `Klipo.app` in Applications, choose **Open**, then choose **Open** again in the confirmation dialog.
4. If macOS still blocks Klipo, go to **System Settings > Privacy & Security**, choose **Open Anyway** for Klipo, then confirm **Open**.
5. If Klipo remains blocked after those steps and you downloaded it from the official GitHub release, remove the quarantine attribute only from the installed app:

   ```sh
   xattr -dr com.apple.quarantine "/Applications/Klipo.app"
   ```

   Do not disable Gatekeeper globally.
6. Launch Klipo.
7. Enable Klipo under **System Settings > Privacy & Security > Accessibility**.
8. Restart Klipo.

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
