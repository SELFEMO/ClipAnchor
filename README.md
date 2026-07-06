<div align="center">
  <img src="src-tauri/icons/128x128.png" width="92" height="92" alt="ClipAnchor logo" />
  <h1>ClipAnchor · 剪贴锚</h1>
  <p><strong>A portable, quiet, pinnable clipboard workspace for modern desktops</strong></p>
  <p><a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a></p>
</div>

## Overview

ClipAnchor is a cross-platform clipboard pinning tool built with Rust, Tauri, and React. It monitors copied text, images, and files in the background, turns them into compact desktop popups, and saves non-sensitive content into local history. Important items can be favorited, pinned again, copied back to the clipboard, searched, and managed from the history list.

## AI development notice

This project was implemented with AI-assisted programming. Before public release or production use, review the code, test the target platforms, verify the watermark behavior with your own sample set, and confirm all third-party binary licenses.

## Current Verification Status

| Platform      | Current Status | Notes |
| ------------- | -------------- | ----- |
| Windows x64   | Verified       | -     |
| Windows ARM64 | Unverified     | -     |
| macOS ARM64   | Unverified     | -     |
| macOS x64     | Unverified     | -     |
| Linux x64     | Unverified     | -     |
| Linux ARM64   | Unverified     | -     |

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
npm install --registry=https://registry.npmmirror.com
npm run desktop:dev
```

On Windows, install Rust, Node.js, Microsoft Visual Studio Build Tools, and WebView2 Runtime before development. The project includes `.cargo/config.toml`, so Cargo uses a sparse mirror by default, which helps in networks where crates.io is unstable.


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

Tauri writes installers to `src-tauri/target/release/bundle/`. The project scripts copy distributable artifacts into the root `release/` folder for easier publishing.

Linux targets are DEB and RPM. Windows targets include NSIS and MSI installers. macOS targets include APP and DMG.

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
