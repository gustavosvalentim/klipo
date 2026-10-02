# Klipo

Klipo is a macOS clipboard manager built with Tauri, React, TypeScript, and Rust. It stores recent text and image clipboard entries and provides a floating picker for selecting and pasting them.

## Features

- Text and image clipboard history
- Persistent history stored locally
- Global shortcut to open the picker
- Configurable picker shortcuts
- Menu bar resident mode
- Rotating JSON diagnostic logs

Klipo supports macOS and has an existing Linux X11/Wayland implementation. X11 supports global shortcuts and automatic paste when native integrations are available. Wayland uses manual paste and has watcher limitations. Windows support is not implemented.

### X11 clipboard ownership

When Klipo runs in an X11 session, `clipboard-rs` owns the X11 clipboard selection for as long
as the Klipo process remains alive. Klipo verifies every write before attempting a paste, including
text, images, and mixed content (with a text-only fallback when the desktop rejects mixed data).
X11 selection ownership is process-lifetime: if Klipo exits, its copied content is no longer
available unless an external clipboard manager has persisted it. Automatic paste remains disabled
on Wayland and when the X11 target-restoration adapter cannot be probed.

The X11 implementation has protocol-level unit tests, but native acceptance on Ubuntu 22.04 and
24.04 has not been run from this Darwin ARM64 development host.

## Install

> [!WARNING]
> Klipo's macOS app has an ad-hoc signature and is not notarized. Download it only from the official [GitHub Releases page](https://github.com/gustavosvalentim/klipo/releases).

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

Accessibility permission is required to paste into the previously active application. If Klipo
cannot activate that application or simulate the paste shortcut, it attempts to restore the picker and
reports that the item was copied so you can paste it manually. Recovery failures are reported separately.

## Usage

- `Cmd+Shift+V`: open the picker
- `Up` / `Down`: navigate entries
- `Enter`: paste the selected entry
- `Delete`: remove the selected entry
- `Esc`: close the picker

Open **Settings** from the menu bar to change the picker shortcuts. Only the shortcut for opening Klipo is global.

## Development

Requirements:

- macOS or Linux with the Tauri system dependencies
- Rust and Cargo
- Bun

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
bun run test
bun run build
```

Frontend behavior is tested with Vitest. Rust unit tests run with `cargo test --manifest-path src-tauri/Cargo.toml`. Passing unit tests does not establish native desktop acceptance; the Linux checks use Xvfb, and manual macOS/X11/Wayland acceptance remains separate.

As of 2026-10-02, a macOS ARM64 Tauri release binary builds successfully. Interactive macOS picker/paste acceptance and native Ubuntu Xorg/Wayland acceptance for the architecture simplification remain outstanding.

Format the project:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml
bun run format
```
