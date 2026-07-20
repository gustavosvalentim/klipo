# 📋 Klipo

Klipo is a small clipboard history app built with Tauri, React, TypeScript, and Rust.

It runs as a lightweight desktop utility: copy text normally, open Klipo with a global shortcut, choose a previous clipboard item, and paste it back into the app you were using.

## 🍎 Platform support

Klipo is currently macOS-focused.

Several parts of the app depend on macOS-specific behavior, including private Tauri macOS APIs, AppKit window focus handling, accessory app mode, a translucent floating window, and simulated paste behavior using the macOS command key.

Linux and Windows support is TBD. Some code paths exist for those platforms, but the app should be treated as macOS-only until cross-platform behavior is implemented and tested.

## ✨ Features

- Text clipboard history.
- Global shortcut to open the picker at the cursor position.
- Floating, always-on-top clipboard menu.
- Keyboard navigation for selecting, deleting, and pasting items.
- Tray icon so the app can stay resident in the background.
- In-memory history capped at 120 text items.

## ⌨️ Usage

- `Ctrl+Alt+V`: show Klipo at the current cursor position.
- `ArrowUp` / `ArrowDown`: move through clipboard items.
- `ArrowRight`: focus the delete action for the selected item.
- `ArrowLeft`: return focus to the selected clipboard item.
- `Enter`: paste the selected item, or delete it when the delete action is active.
- `Escape`: hide Klipo.

Klipo currently stores text only. Image clipboard support is not implemented yet, and history is not persisted across app restarts.

## 🗺️ Roadmap

The following work is planned to evolve Klipo from a macOS text clipboard utility into a cross-platform clipboard manager.

- [ ] **Image support** — Capture image clipboard entries alongside text, store the image data and metadata safely, show thumbnails in the picker, and restore the selected image to the system clipboard before pasting.
- [ ] **Persistent history between sessions** — Save clipboard history to an application-data store, restore it at launch, and ensure clearing or deleting an item updates the stored history. This will include sensible limits for retained items and stored data size.
- [ ] **File logging** — Write application events, errors, and platform-integration diagnostics to rotating log files so issues can be investigated after Klipo has been running.
- [ ] **Linux support** — Replace macOS-specific window-focus and paste behavior with Linux-compatible implementations, verify clipboard monitoring and global shortcuts across the supported desktop environments, and provide Linux build/install artifacts.
- [ ] **Windows support** — Implement Windows focus restoration and paste behavior, validate the picker, tray, global shortcut, and clipboard monitoring on Windows, and provide Windows release artifacts.

Cross-platform support depends on platform-specific focus restoration and input simulation: the current paste flow only fully supports macOS.

## 📦 Install on macOS

1. Open the [GitHub Releases page](https://github.com/gustavosvalentim/klipo/releases) and download the `.dmg` file for the latest release.
2. Open the downloaded DMG, then drag **Klipo** into the **Applications** folder.
3. Launch Klipo from Applications.
4. Open **System Settings → Privacy & Security → Accessibility**, unlock the settings if prompted, and enable **Klipo**.
5. Quit Klipo completely and open it again.

Restarting Klipo after granting Accessibility access is a temporary workaround. It is currently needed for Klipo to simulate paste into the app that was previously active.

## 🛠️ Development

Prerequisites:

- macOS.
- Rust and Cargo.
- Bun.
- Tauri development dependencies for macOS.

<details>
<summary>Setup</summary>

```sh
bun install
```

</details>

<details>
<summary>Run the app locally</summary>

Using Bun:

```sh
bun run tauri dev
```

Using Cargo:

```sh
cargo tauri dev
```

Run only the frontend dev server:

```sh
bun run dev
```

</details>

<details>
<summary>Generate release binaries</summary>

Using Bun:

```sh
bun run tauri build
```

Using Cargo:

```sh
cargo tauri build
```

The generated app bundle and installer artifacts are written under `src-tauri/target/release/bundle/`.

Build only the frontend assets:

```sh
bun run build
```

</details>

<details>
<summary>Format the project</summary>

Format Rust code:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Format frontend code:

```sh
bun run format
```

</details>

## 🔐 macOS permissions

Because Klipo listens for global shortcuts, tracks the clipboard, restores focus to the previous app, and simulates paste, macOS may require permissions such as Accessibility or Input Monitoring depending on your system settings.

If the picker opens but cannot paste back into another app, check macOS System Settings privacy permissions for the built Klipo app or the development terminal running it.
