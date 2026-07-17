# ClipAnchor Extension Language Pack Guide

This guide explains how to create a language pack manually.

## 1. Use the English source as the authority

Open:

```text
src/locales/en.js
```

Copy every key in its `messages` object. Translate only the string values.

## 2. Start from the template

Copy:

```text
docs/i18n/language-pack.template.json
```

Rename it to a BCP 47-style language tag, for example:

```text
fr.json
pt-BR.json
sr-Latn.json
zh-TW.json
```

Use `-`, not `_`.

## 3. Keep placeholders unchanged

The following tokens are inserted at runtime and must not be translated or removed:

```text
{language}
{count}
{error}
{days}
{size}
```

A translated sentence may move the placeholder to a natural position, but the placeholder spelling must remain identical.

## 4. Validate JSON

Requirements:

- UTF-8 encoding
- no comments
- no trailing commas
- double-quoted keys and strings
- valid escape sequences
- `messages` contains string-to-string pairs

## 5. Install the pack

Copy the completed file to:

```text
data/locales/
```

Fully quit ClipAnchor, including the tray process, and restart it.

## 6. Update detection

ClipAnchor compares:

- missing keys;
- the current English-source fingerprint with `source_hash`;
- the current translation fingerprint with `translation_hash`;
- retired keys that no longer exist in English.

When an external edit changes a translation, ClipAnchor marks the entry as manually modified so an automatic incremental update does not silently overwrite it.
