<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="ClipAnchor logo" width="128" />
</p>

<h1 align="center">ClipAnchor · 剪贴锚</h1>

<p align="center">
  A portable, quiet, pinnable clipboard workspace for modern desktops
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">简体中文</a>
</p>

## Overview

ClipAnchor is a cross-platform clipboard pinning tool built with Rust, Tauri, and React. It monitors copied text, images, and files in the background, turns them into compact desktop popups, and saves non-sensitive content into local history. Important items can be favorited, pinned again, copied back to the clipboard, searched, and managed from the history list.

ClipAnchor is designed to stay **portable** and **quiet**. Runtime data is stored beside the application under `data/`, which makes backup and migration straightforward. When launched at system startup, ClipAnchor enters Lite mode by default: no main window is shown, while the tray icon, clipboard monitor, and database service keep running silently.

> This project is still undergoing continuous development. Before downloading, installing, or upgrading, we recommend that you read the release notes and back up your important data.

## AI development notice

This project was implemented with AI-assisted programming.

Before public release or production use, review the code, test every target platform, verify clipboard capture, popup, history, autostart, installer, and update behavior with your own sample set, and confirm all third-party binary licenses.

## Current Verification Status

| Platform | Status | Notes |
|---|---|---|
| Windows x64 | Verified | Basic desktop, tray, clipboard, history, update, and CLI smoke tests passed. |
| Windows ARM64 | Pending | Needs real-device package and runtime verification. |
| macOS ARM64 | Verified | Apple Silicon APP/DMG runtime smoke tests passed. |
| macOS x64 | Pending | Needs Intel macOS package and runtime verification. |
| Linux x64 | Pending | Needs DEB/RPM package and desktop-environment verification. |
| Linux ARM64 | Pending | Needs ARM64 Linux package and runtime verification. |

This table is a compact release checklist. Re-test long-running background behavior, autostart Lite mode, installer handoff, and update delivery before publishing a new build.

## Features

| Area | Capability |
|---|---|
| Pinned popups | Creates an independent desktop popup for each copy action, with Pin, Copy, Unpin, auto-destroy, drag, and smart stacking. |
| Clipboard types | Supports text, images, files, and mixed clipboard content. |
| Lite mode | Startup launch runs silently without showing the main window; the UI can be restored from the tray or a shortcut. |
| Single instance | Relaunching ClipAnchor activates and foregrounds the existing main window instead of leaving another long-running process. |
| History | Local SQLite history with search, type filters, text editing, single delete, batch delete, and pin-from-history. |
| Favorites | Favorite items are shown separately and remain in normal history; normal cleanup keeps them by default. |
| Privacy filter | Off, light, and smart modes; light mode uses local rules to detect common sensitive-content patterns. |
| Shortcuts | Global actions for the pin service, history service, show/hide main window, Lite mode, and pause/resume monitoring. |
| Data tools | Import/export JSON or metadata-complete CSV history, clean records by age, show the database location, and manage rotated runtime logs. |
| Appearance | Dark, light, and system themes, UI scale, popup scale, transient scrollbars, and animation controls. |
| Localization | Built-in English and Simplified Chinese, plus local extension language packs with incremental updates. |
| Portable data | History, settings, resources, exports, language packs, updates, and logs stay inside `data/`. |

## Download and installation

Published installers and portable archives are available on the [Releases](https://github.com/SELFEMO/ClipAnchor/releases) page.

1. Open the latest release.
2. Download an asset that matches the operating system and CPU architecture.
3. Install the package or extract the portable archive.
4. Start ClipAnchor and configure shortcuts, privacy filtering, appearance, language, and cleanup behavior in Settings.

When no compatible package is available, build ClipAnchor from source. Back up important files under `data/` before upgrading or replacing a portable installation.

## Quick start

### Requirements

- Git
- Node.js and npm
- Rust toolchain
- Platform-specific dependencies required by Tauri
- Microsoft Visual Studio Build Tools and WebView2 Runtime for Windows development
- The appropriate native build environment for macOS or Linux packaging

### Development

```bash
git clone https://github.com/SELFEMO/ClipAnchor.git
cd ClipAnchor
npm install --registry=https://registry.npmmirror.com
npm run desktop:dev
```

Run `npm install` and `npm run desktop:dev` inside the cloned `ClipAnchor` directory. Running them from the parent directory causes a `Could not read package.json` error.

Use the following command when a clean rebuild is needed:

```bash
npm run clean
```

The project includes `.cargo/config.toml`, so Cargo uses the configured sparse registry settings when resolving Rust dependencies.

### Check the installed version

```powershell
clipanchor.exe --version
clipanchor.exe -V
```

On macOS and Linux, pass the same flags to the installed `clipanchor` binary. The command prints the application version and exits without opening the main window or starting clipboard monitoring.

### Build installers

```bash
npm run desktop:build
```

Target-specific build commands currently include:

```bash
npm run desktop:build:windows-x64
npm run desktop:build:macos-arm64
npm run desktop:build:macos-x64
```

For Apple Silicon on a macOS host, add the Rust target before the first ARM64 build:

```bash
rustup target add aarch64-apple-darwin
npm run desktop:build:macos-arm64
```

Tauri writes installers to `src-tauri/target/release/bundle/` or `src-tauri/target/<target-triple>/release/bundle/` for target-specific builds. Project scripts then collect distributable artifacts into the root `release/` folder.

Linux targets include DEB and RPM. Windows targets include NSIS and MSI. macOS targets include APP and DMG. Create macOS DMG files on macOS, then sign and notarize them with the maintainer's own Apple Developer credentials before public distribution.

## Basic usage

1. Start ClipAnchor and make sure **Pin Service** and **History Service** are enabled.
2. Copy text, images, or files to create compact desktop popups.
3. Select **Pin** to keep a popup above other windows and reveal actions such as **Copy** and **Unpin**.
4. Search history on the Clipboard page, favorite important records, edit text entries, or pin an existing record again.
5. Use Settings to adjust theme, language, scale, shortcuts, popup position, privacy filtering, auto-destroy delay, and cleanup behavior.
6. After enabling launch at startup, ClipAnchor signs in silently in Lite mode. Double-click the tray icon, choose **Show ClipAnchor**, or press `Ctrl+Shift+X` to restore the main window.

## Extension language packs

ClipAnchor includes English and Simplified Chinese. Other languages can be loaded from local JSON extension packs.

### Translation references

Use these files when creating a language pack:

- English source messages and authoritative keys: [`src/locales/en.js`](src/locales/en.js)
- Public JSON template: [`docs/i18n/language-pack.template.json`](docs/i18n/language-pack.template.json)
- Detailed translation guide: [`docs/i18n/README.md`](docs/i18n/README.md)
- Optional validator: [`scripts/validate-language-pack.mjs`](scripts/validate-language-pack.mjs)

Copy the keys from the `messages` object in `src/locales/en.js` and translate the values only. Do not translate, delete, or arbitrarily rename message keys.

### Minimum compatible structure

```json
{
  "format": "clipanchor-language-pack",
  "code": "fr",
  "label": "French",
  "native_name": "Français",
  "source": "manual",
  "source_locale": "en",
  "messages": {
    "settings": "Paramètres",
    "cancel": "Annuler",
    "ok": "OK"
  },
  "message_status": {}
}
```

This example demonstrates the structure only. A distributable pack should contain every current UI key from `src/locales/en.js`.

Language-pack requirements:

- Save the file as UTF-8.
- Use valid JSON without comments or trailing commas.
- Keep `messages` as an object with string keys and string values.
- Preserve placeholders such as `{language}`, `{count}`, `{error}`, and `{days}`.
- Preserve JSON escapes such as `\n` and `\"`.
- Do not store API keys, clipboard content, or other private data in a language pack.
- `message_status` may initially be empty for a manually created pack.

### Filename and language-code standard

Use an IETF BCP 47-style language tag as the filename:

| Language | Filename |
|---|---|
| Japanese | `ja.json` |
| French | `fr.json` |
| German | `de.json` |
| Spanish | `es.json` |
| Brazilian Portuguese | `pt-BR.json` |
| Traditional Chinese, Taiwan | `zh-TW.json` |
| Serbian, Latin script | `sr-Latn.json` |

Naming rules:

- Use hyphens (`-`), not underscores (`_`).
- Primary language subtags are normally lowercase, such as `fr` and `ja`.
- Script subtags use title case, such as `Latn` and `Hant`.
- Region subtags are normally uppercase, such as `BR` and `TW`.
- Do not use spaces or display names such as `French.json`.
- The JSON `code` should match the filename without `.json`.
- `auto`, `en`, `en-*`, `zh`, `zh-CN`, and `zh-Hans*` are reserved for automatic or built-in language handling.

### Install a language pack

Copy the completed JSON file to:

```text
data/locales/
```

Fully quit ClipAnchor, including the tray process, and restart it. The language will then appear in the extension-language list in Settings.

### Validate a language pack

From the project root:

```bash
node scripts/validate-language-pack.mjs data/locales/fr.json
```

The validator checks the filename, JSON structure, missing keys, changed source entries, manually modified translations, and removed keys.

### Incremental update rules

ClipAnchor compares each extension pack with the current English messages:

| State | Detection | Result |
|---|---|---|
| Missing entry | The English key exists but is absent from `messages` | Add it to the update set. |
| Changed source | Current English text does not match the stored `source_hash` | Mark the translation as outdated. |
| Removed entry | The pack contains a key no longer present in English | Remove it locally without calling a translation API. |
| Manual edit | Current translation does not match the stored `translation_hash` | Set `modified: true` and preserve the human-edited value. |
| Damaged file | JSON cannot be parsed, the structure is invalid, or no usable messages exist | Mark the pack as corrupt and require repair or regeneration. |

`source_hash` and `translation_hash` are lightweight change fingerprints, not cryptographic security hashes.

The incremental-update policy is:

1. Translate only missing entries and entries whose English source changed.
2. Reuse unchanged translations.
3. Remove retired keys locally.
4. Preserve detected human edits rather than overwriting them automatically.
5. Manually review a human-edited entry when its corresponding English source later changes.

### Troubleshooting

- **Update available** means the pack remains usable but has missing, changed, or retired keys.
- **File error/corrupt** means the pack cannot be safely loaded.
- When a language does not appear, confirm that the file is under `data/locales/`, uses `.json`, has a valid language-tag filename, and contains valid JSON.
- Fully quit the tray process before restarting ClipAnchor.
- English fallback text usually indicates missing message keys.
- Broken runtime text usually indicates that an original `{...}` placeholder was changed or removed.

## Data location

ClipAnchor stores runtime data beside the application:

```text
data/
├── clipanchor.db
├── settings.json
├── locales/
├── resources/
├── exports/        # JSON and CSV history exports include record metadata.
├── updates/
└── logs/
    ├── clipanchor.log
    └── clipanchor-*.log
```

Logs rotate automatically when the active file grows too large. Settings → Log management provides log-size information, configurable retention, access to the log directory, refresh, and cleanup controls.

Move the whole installation folder to migrate history and settings together. Back up important data before deleting or replacing `data/`.

## Privacy and data safety

ClipAnchor processes copied text, images, and file paths.

- Avoid retaining sensitive clipboard content on shared devices.
- Configure privacy filtering, content-type filters, and cleanup rules for the intended workflow.
- Sanitize logs and screenshots before publishing an issue.
- Never commit `data/`, `.env` files, API keys, signing certificates, private keys, or local build credentials.
- Back up records that must be retained before clearing local data.

## Update channel

ClipAnchor can silently check GitHub Releases at startup when **Auto update** is enabled in Settings. Startup checks do not open the update card for checking, no-update, generic failures, or releases without a compatible package.

A foreground prompt appears only when an update is actionable, such as when a compatible package is ready to install or automatic download failed and the user must choose whether to open the release asset.

The manual **Check update** button opens an in-app status card immediately and can show checking, downloading, ready-to-install, no-update, incompatible-asset, or failure states. Release tags should use `pre-release-v...` or `release-v...`.

Asset selection is automatic:

- Windows prefers `ClipAnchor_Windows_x64.exe`; when no EXE exists, it selects a matching MSI such as `ClipAnchor_Windows_x64_zh-CN.msi` or `ClipAnchor_Windows_x64_en-US.msi`.
- macOS uses DMG and filters architecture-specific filenames so Apple Silicon selects ARM64 or universal packages rather than Intel-only packages.
- Linux selects DEB or RPM according to the distribution family.

Before each check, old packages under `data/updates/` are removed so stale installers are not reused accidentally. Newly downloaded packages are stored in `data/updates/` and opened through the system installer when the user chooses **Install now**.

## Project layout

| Path | Purpose |
|---|---|
| `src/index.html` | Vite entry for the desktop application. Production builds emit `dist/index.html` for the Tauri main and popup windows. |
| `src/` | React frontend, including the main shell, Clipboard page, Settings page, popup page, API wrapper, localization, hooks, and global styles. |
| `src/locales/` | Built-in English and Simplified Chinese message sources. |
| `src-tauri/` | Rust/Tauri backend for clipboard monitoring, database access, tray, autostart, shortcuts, single instance, updates, and window control. |
| `data/` | Portable runtime data for the database, settings, language packs, resources, exports, updates, and logs. |
| `docs/index.html` | Standalone website for GitHub Pages or static hosting; it is not part of the desktop runtime. |
| `docs/i18n/` | Public extension-language documentation and JSON template. |
| `scripts/` | Development, validation, cleanup, release collection, and portable-package scripts. |
| `release/` | Distribution folder populated with installers and portable archives after a build. |

## Release artifact names

Release scripts try to organize installers with names such as:

```text
ClipAnchor_Windows_x64.msi
ClipAnchor_Windows_x64.exe
ClipAnchor_macOS_arm64.dmg
ClipAnchor_Linux_x64.deb
ClipAnchor_Linux_x64.rpm
```

Actual output depends on the host operating system, CPU architecture, and installed Tauri bundling toolchain.

## Reporting issues

A useful issue report includes:

- operating system and version;
- ClipAnchor version;
- clear reproduction steps;
- expected and actual behavior;
- sanitized logs;
- screenshots or recordings when necessary.

Do not publish clipboard content, access tokens, private paths, or other sensitive information.

## Contributing

1. Fork the repository and create a focused branch.
2. Install dependencies and verify that development mode starts.
3. Keep English and Simplified Chinese message keys synchronized.
4. Update both built-in language files whenever UI text changes.
5. Verify extension-pack incremental updates after adding or changing English keys.
6. Run the available build, formatting, and static checks before committing.
7. Open a pull request with a clear change summary and validation notes.

Recommended commit style:

```text
feat: add clipboard filter
fix: preserve manually edited translations
docs: improve language-pack guide
```

## License

ClipAnchor is licensed under the Apache License 2.0. See the root [`LICENSE`](LICENSE) file for the full license text.
