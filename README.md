# ClipAnchor

English | [简体中文](README.zh-CN.md)

ClipAnchor is a desktop clipboard companion built with **Tauri + React**. It focuses on clipboard previews, pinned floating content, history, favorites, privacy filtering, shortcuts, and extensible local language packs. The application is designed primarily around local execution and local data storage.

> ClipAnchor is under active development. Before installing or upgrading, review the release notes and back up any important local data.

## Features

- Text, image, file, and mixed clipboard content.
- Lightweight clipboard preview windows with pin, unpin, and close actions.
- Searchable clipboard history, favorites, deletion, and cleanup.
- Editable text records and history import/export.
- Privacy mode, content-type filters, and configurable auto-destroy delay.
- Global shortcuts, light/dark/system themes, and UI scaling.
- Autostart, tray operation, remembered window position, and update checks.
- Built-in English and Simplified Chinese.
- Local extension language packs with incremental update support.
- Reuse of unchanged translations to reduce unnecessary translation API calls.

## Download and Install

### Use a release build

1. Open the repository's **Releases** page.
2. Download the package that matches your operating system and CPU architecture.
3. Install or extract the package, then start ClipAnchor.
4. Use Settings to configure shortcuts, privacy filters, theme, language, and popup lifetime.

When no package is published for a platform, build the application from source.

### Portable data directory

A portable build normally stores runtime data beside the application:

```text
data/
├── locales/     # User extension language packs
├── logs/        # Logs
└── ...          # Settings, history, and other runtime data
```

Do not publish the `data/` directory. It may contain personal settings, logs, or clipboard data.

## Run from Source

### Requirements

- Node.js and npm
- Rust toolchain
- System build dependencies required by Tauri
- Git

Platform-specific system dependencies differ. Configure a working Tauri desktop development environment before building.

### Install dependencies

```bash
npm install
```

### Development mode

```bash
npm run tauri:dev
```

### Build release packages

```bash
npm run tauri:build
```

If script names change, use the current entries in `package.json`.

## Basic Usage

1. Start ClipAnchor.
2. Enable the clipboard pin service and history service in Settings.
3. Copy text, an image, or files.
4. Pin the preview when it should remain visible, or let an unpinned preview close automatically.
5. Search, favorite, copy, edit, or delete records in the main window.
6. Configure shortcuts, appearance, language, scaling, filters, and retention rules in Settings.

## Extension Language Packs

### Built-in and extension languages

Built-in language ranges:

- `en`: English
- `zh-CN` / `zh-Hans`: Simplified Chinese

Other languages are loaded from JSON extension packs. Place each pack in:

```text
data/locales/
```

After copying the file, **fully quit and restart ClipAnchor**. The language will then appear under Settings → Appearance → Extension languages.

### Files to use as references

- Authoritative English keys and source text: [`src/locales/en.js`](src/locales/en.js)
- JSON template: [`docs/i18n/language-pack.template.json`](docs/i18n/language-pack.template.json)
- Detailed guide: [`docs/i18n/README.md`](docs/i18n/README.md)

Use the keys in the `messages` object from `src/locales/en.js`. **Translate values only. Do not translate, delete, or arbitrarily rename keys.**

### File names and language tags

A language-pack filename must use an **IETF BCP 47-style** language tag and the `.json` extension.

Examples:

| Language | Filename |
|---|---|
| Japanese | `ja.json` |
| French | `fr.json` |
| German | `de.json` |
| Spanish | `es.json` |
| Brazilian Portuguese | `pt-BR.json` |
| Traditional Chinese, Taiwan | `zh-TW.json` |
| Serbian, Latin script | `sr-Latn.json` |

Rules:

- Use hyphens (`-`), not underscores (`_`).
- Primary language subtags are normally lowercase, such as `fr` and `ja`.
- Script subtags use title case, such as `Latn` and `Hant`.
- Region subtags are normally uppercase, such as `BR` and `TW`.
- Do not use spaces or display names in filenames.
- The JSON `code` should match the filename without `.json`.
- `auto`, `en`, `en-*`, `zh`, `zh-CN`, and `zh-Hans*` are reserved for automatic or built-in language handling and should not be used as ordinary extension-pack names.

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

A real pack should contain every UI key from `src/locales/en.js`. The example above only demonstrates the structure and is not a complete production pack.

Requirements:

- Save the file as UTF-8.
- Use valid JSON: no comments and no trailing commas.
- `messages` must be an object with string keys and string values.
- Preserve placeholders such as `{language}`, `{count}`, `{error}`, and `{days}`.
- Preserve escape semantics. Use `\n` for a newline and `\"` for a quotation mark inside JSON strings.
- Never store API keys, clipboard data, or other private information in a language pack.
- `message_status` may initially be empty. ClipAnchor can establish compatibility metadata when it first scans a legacy or manually created pack.

### How ClipAnchor decides whether an entry needs an update

ClipAnchor compares every extension-language entry with the current English reference messages:

| State | Detection | Result |
|---|---|---|
| Missing entry | The English key exists but is absent from `messages` | Add to the incremental update set |
| Changed source | The current English text hash differs from stored `source_hash` | Mark as outdated |
| Removed entry | The pack contains a key no longer present in English | Remove locally during update without calling a translation API |
| Manual edit | Current translation hash differs from stored `translation_hash` | Set `modified: true` to protect the human-edited value |
| Damaged file | JSON cannot be parsed, the structure is invalid, or no usable messages exist | Mark as corrupt and require repair/regeneration |

`source_hash` and `translation_hash` are lightweight change fingerprints, not cryptographic security hashes.

Incremental-update policy:

1. Translate only missing entries and entries whose English source changed.
2. Reuse unchanged translations.
3. Remove retired keys locally.
4. Preserve detected human edits instead of overwriting them automatically.
5. When the English source of a human-edited entry later changes, the translator should manually review the entry for semantic compatibility.

## Language-Pack Troubleshooting

- **Update available** means the pack is still usable but contains missing, changed, or removed keys.
- **File error/corrupt** means the JSON cannot be safely loaded and should be repaired or regenerated.
- Language does not appear:
  1. Confirm the file is in `data/locales/`.
  2. Confirm the extension is `.json`.
  3. Confirm the filename is a valid language tag.
  4. Validate the JSON.
  5. Fully quit ClipAnchor from the tray and restart it.
- English fallback text usually means that one or more keys are missing.
- Broken placeholders usually mean that an original `{...}` token was changed or removed.

## Project Layout

```text
ClipAnchor/
├── src/                    # React frontend
│   ├── locales/            # Built-in languages
│   ├── pages/              # Main pages
│   └── popup/              # Clipboard preview window
├── src-tauri/              # Rust / Tauri backend
├── scripts/                # Build and helper scripts
├── docs/                   # Documentation and publishable static content
├── data/                   # Local runtime data; do not publish
├── README.md
└── README.zh-CN.md
```

## Privacy and Data Safety

ClipAnchor processes copied text, images, and file paths. Keep the following in mind:

- Avoid retaining sensitive clipboard content on shared devices.
- Enable privacy filters and cleanup rules that match your workflow.
- Remove private paths, tokens, and clipboard text before publishing logs or issues.
- Never commit `data/`, `.env` files, API keys, signing certificates, or local build credentials.
- Back up records that must be retained before deleting local data.

## Reporting Issues

A useful issue report includes:

- Operating system and version.
- ClipAnchor version.
- Reproduction steps.
- Expected and actual behavior.
- Sanitized logs.
- Screenshots or recordings when necessary.

Do not publish clipboard contents, access tokens, private paths, or other sensitive information.

## Contributing

1. Fork the repository and create a focused branch.
2. Install dependencies and verify that development mode starts.
3. Keep English and Simplified Chinese message keys synchronized.
4. Update both built-in languages when UI copy changes.
5. Verify incremental extension-pack behavior after adding English keys.
6. Run available build, formatting, and static checks before committing.
7. Open a pull request with a clear change summary and validation notes.

Recommended commit style:

```text
feat: add clipboard filter
fix: preserve manually edited translations
docs: improve language-pack guide
```

## License

See the repository's [`LICENSE`](LICENSE) file for the applicable license terms.
