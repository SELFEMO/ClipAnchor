<div align="center">
  <img src="src-tauri/icons/128x128.png" width="92" height="92" alt="ClipAnchor logo" />
  <h1>ClipAnchor · 剪贴锚</h1>
  <p><strong>A portable, quiet, pinnable clipboard workspace for modern desktops</strong></p>
  <p><a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a></p>
</div>

## Overview

ClipAnchor is a cross-platform clipboard pinning tool built with Rust, Tauri, and React. It monitors copied text, images, and files in the background, turns them into compact desktop popups, and saves non-sensitive content into local history. Important items can be favorited, pinned again, copied back to the clipboard, searched, and managed from the history list.

## AI development notice

This project was implemented with AI-assisted programming. Before public release or production use, review the code, test every target platform, verify clipboard capture, popup, history, autostart, installer, and update behavior with your own sample set, and confirm all third-party binary licenses.

## Current Verification Status

| Platform      | Status | Notes |
| ------------- | ------ | ----- |
| Windows x64   | Verified | Basic desktop, tray, clipboard, history, update, and CLI smoke tests passed. |
| Windows ARM64 | Pending | Needs real-device package and runtime verification. |
| macOS ARM64   | Verified | Apple Silicon APP/DMG runtime smoke tests passed. |
| macOS x64     | Pending | Needs Intel macOS package and runtime verification. |
| Linux x64     | Pending | Needs DEB/RPM package and desktop-environment verification. |
| Linux ARM64   | Pending | Needs ARM64 Linux package and runtime verification. |

This table is a compact release checklist. Re-test long-running background behavior, autostart Lite mode, installer handoff, and update delivery before publishing a new build.

ClipAnchor is designed to stay portable and quiet. Runtime data is stored beside the application under `data/`, which makes backup and migration straightforward. When launched at system startup, ClipAnchor enters Lite mode by default: no main window is shown, while the tray icon, clipboard monitor, and database service keep running silently.

## Project layout

| Path | Purpose |
|---|---|
| `src/index.html` | Vite entry for the desktop app. The source entry lives under `src/`; production builds still emit `dist/index.html` for Tauri main and popup windows. |
| `src/` | React frontend, including the main shell, clipboard page, settings page, popup page, API wrapper, and global styles. |
| `src-tauri/` | Rust/Tauri backend for clipboard monitoring, database access, tray, autostart, shortcuts, single instance, and window control. |
| `data/` | Portable data directory for the database, settings, resources, exports, and logs. |
| `docs/index.html` | Standalone website for GitHub Pages or static hosting. It is not part of the desktop runtime. |
| `scripts/` | Scripts for release collection and portable package creation. |
| `release/` | Distribution folder where installers and portable archives are copied after build. |

## Features

| Area | Capability |
|---|---|
| Pinned popups | Creates an independent desktop popup for every copy action, with Pin, Copy, Unpin, auto-destroy, drag, and smart stacking. |
| Lite mode | Startup launch runs silently without showing the main window; the UI can be restored from tray or shortcut. |
| Single instance | Relaunching ClipAnchor activates and foregrounds the existing main window instead of leaving another long-running process. |
| History | Local SQLite history with search, type filters, single delete, batch delete, and pin-from-history. |
| Favorites | Favorite items are shown separately and still remain in normal history; normal cleanup keeps them by default. |
| Privacy filter | Off, light, and smart modes; light mode uses local rules to detect common sensitive content patterns. |
| Shortcuts | Global actions for pin service, history service, show/hide main window, Lite mode, and pause/resume monitoring. |
| Data tools | Import/export JSON or metadata-complete CSV history, clean records by age, show database location, and manage rotated runtime logs. |
| Appearance | Dark, light, and system themes, UI scale, popup scale, and animation mode. |
| Portable data | History, settings, resources, exports, and logs stay inside `data/`. |

## Quick start

### Development

```bash
git clone https://github.com/SELFEMO/ClipAnchor.git
cd ClipAnchor
npm install --registry=https://registry.npmmirror.com
npm run desktop:dev
```

Run `npm install` and `npm run desktop:dev` inside the cloned `ClipAnchor` directory. The command fails with `Could not read package.json` if it is executed from the parent folder. Use `npm run clean` when a clean rebuild is needed; it replaces shell-specific `rm -rf` cleanup and avoids intermittent `Directory not empty` failures on macOS. On Windows, install Rust, Node.js, Microsoft Visual Studio Build Tools, and WebView2 Runtime before development. The project includes `.cargo/config.toml`, so Cargo uses a sparse mirror by default, which helps in networks where crates.io is unstable.


### Check installed version

```powershell
clipanchor.exe --version
clipanchor.exe -V
```

On macOS and Linux, use the same flags with the installed `clipanchor` binary. The command prints the application version and exits immediately, so it does not open the main window or start the clipboard service.

### Build installers

```bash
npm run desktop:build
```

For Apple Silicon on a macOS host, add the target once and build the ARM64 bundle:

```bash
rustup target add aarch64-apple-darwin
npm run desktop:build:macos-arm64
```

Tauri writes installers to `src-tauri/target/release/bundle/` or `src-tauri/target/<target-triple>/release/bundle/` for target-specific builds. The project scripts copy distributable artifacts into the root `release/` folder for easier publishing.

Linux targets are DEB and RPM. Windows targets include NSIS and MSI installers. macOS targets include APP and DMG. macOS DMG creation should be run on macOS, then signed and notarized with your own Apple Developer certificate before public distribution.

## Basic usage

1. Start ClipAnchor and make sure Pin Service and History Service are enabled.
2. Copy text, images, or files to create compact desktop popups.
3. Click Pin to keep a popup above other windows and reveal actions such as Copy and Unpin.
4. Search history in the Clipboard page, favorite important records, or click the Pin icon to create a pinned popup from history.
5. Use Settings to adjust theme, language, scale, shortcuts, popup position, privacy filtering, and cleanup behavior.
6. After enabling launch-at-startup, ClipAnchor signs in silently in Lite mode; double-click the tray icon, choose Show ClipAnchor, or press `Ctrl+Shift+X` to restore the main window.

## Data location

ClipAnchor stores runtime data beside the application:

```text
data/
├── clipanchor.db
├── settings.json
├── resources/
├── exports/        # JSON and CSV history exports include record metadata.
└── logs/
    ├── clipanchor.log
    └── clipanchor-*.log
```

Logs rotate automatically when the current file grows too large. Settings → Log management includes log size, configurable retention days, log folder access, refresh, and cleanup controls.

Move the whole project or installation folder to migrate history and settings together. Back up important data before deleting `data/`.

## Update channel

ClipAnchor can silently check GitHub Releases at startup when **Auto update** is enabled in Settings. Startup checks do not open the update card for checking, no-update, generic failures, or releases that do not contain a compatible package. The foreground prompt appears only when an update is actionable, such as a compatible package is ready to install, or a compatible package exists but automatic download failed and the user must choose whether to open the release asset.

The manual **Check update** button always opens an in-app status card immediately, then shows checking, downloading, ready-to-install, no-update, incompatible-asset, or failure states. Release tags should use `pre-release-v...` or `release-v...`. Asset selection is automatic: Windows prefers `ClipAnchor_Windows_x64.exe`; if no EXE exists, it chooses a localized MSI such as `ClipAnchor_Windows_x64_zh-CN.msi` or `ClipAnchor_Windows_x64_en-US.msi`. macOS uses DMG and now filters architecture-specific names so Apple Silicon selects ARM64 or universal packages instead of Intel-only packages. Linux selects DEB or RPM according to the distribution family. Before each new check, old packages in `data/updates/` are removed so stale installers cannot be reused accidentally. Newly downloaded packages are stored in `data/updates/` and opened through the system installer when the user chooses **Install now**.

## Release artifact names

The release scripts try to organize installers with names like:

```text
ClipAnchor_Windows_x64.msi
ClipAnchor_Windows_x64.exe
ClipAnchor_macOS_arm64.dmg
ClipAnchor_Linux_x64.deb
ClipAnchor_Linux_x64.rpm
```

Actual output depends on the host operating system, CPU architecture, and installed Tauri bundling toolchain.

## License

ClipAnchor is licensed under the Apache License 2.0. See the root `LICENSE` file for the full license text.

## Windows Build Notes

The project does not force a specific Rust registry mirror in `.cargo/config.toml`. This avoids blocking Windows, macOS, and Linux builds when one mirror has DNS or service issues. Configure a mirror in your local Cargo settings or environment only when your network requires it.

