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

| Platform      | Current Status | Notes |
| ------------- | -------------- | ----- |
| Windows x64   | Verified       | Current Windows x64 smoke testing covers desktop startup, tray restore, single-instance activation, settings persistence, clipboard capture/history, manual update checking, GitHub Release downgrade-test detection and package download, old update package cleanup before a new check, hidden background update downloader without command-window flashes, and `--version`. A close-to-tray monitor fix and watchdog have been added for long idle sessions. Before public distribution, repeat long-duration background testing, startup Lite-mode update testing, installer signing, localized MSI UI, and release-side asset publishing verification. |
| Windows ARM64 | Unverified     | Native ARM64 packaging and runtime behavior have not been validated yet. Before release, verify startup, tray, autostart, global shortcuts, clipboard access, and update asset matching on real Windows ARM64 hardware. |
| macOS ARM64   | Unverified     | Build and runtime verification on Apple Silicon are still pending. Before release, validate the APP/DMG bundle, code signing/notarization, clipboard permissions, tray/menu behavior, autostart plist, and update handoff. |
| macOS x64     | Unverified     | Intel macOS packaging and runtime verification are still pending. Before release, validate the same APP/DMG, signing/notarization, permissions, tray/menu, autostart, and update flow on Intel hardware or a reliable Intel environment. |
| Linux x64     | Unverified     | DEB/RPM installation and runtime behavior have not been validated across desktop environments. Before release, test desktop entry creation, tray support, autostart, global shortcuts, X11/Wayland clipboard behavior, and DEB/RPM update selection. |
| Linux ARM64   | Unverified     | Native ARM64 Linux packaging and runtime behavior are still pending. Before release, test DEB/RPM packaging, desktop integration, tray availability, clipboard access, and update asset matching on real ARM64 hardware. |

The status above is a practical packaging and runtime checklist, not a security audit or long-term stability guarantee. **Verified** means the listed platform has passed the current smoke-test scope; **Unverified** means the code is intended to support the platform but still needs a real-device packaging and runtime pass before release. Long-duration background monitoring and release installer handoff should be re-tested after every update to clipboard polling, Lite mode, autostart, installer packaging, signing, or the GitHub Release pipeline.

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

## Update channel

ClipAnchor checks GitHub Releases in the background at startup and keeps the process silent while the main window is closed. The manual **Check update** button opens an in-app status card immediately, then shows checking, downloading, ready-to-install, no-update, incompatible-asset, or failure states. Release tags should use `pre-release-v...` or `release-v...`.

Asset selection is automatic: Windows prefers `ClipAnchor_Windows_x64.exe`; if no EXE exists, it chooses a localized MSI such as `ClipAnchor_Windows_x64_zh-CN.msi` or `ClipAnchor_Windows_x64_en-US.msi`. macOS uses DMG, while Linux selects DEB or RPM according to the distribution family. Before each new check, old packages in `data/updates/` are removed so stale installers cannot be reused accidentally. Newly downloaded packages are stored in `data/updates/` and opened through the system installer when the user chooses **Install now**. A normal manual launch checks in the background and opens the update card if a new version is found; startup Lite mode keeps the check and download silent until the user opens the main window.

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
