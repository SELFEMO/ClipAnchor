#!/usr/bin/env node
/**
 * Static language-pack validator for ClipAnchor.
 *
 * Usage:
 *   node scripts/validate-language-pack.mjs data/locales/fr.json
 */

import fs from "node:fs";
import path from "node:path";

function fail(message) {
  console.error(`ERROR: ${message}`);
  process.exitCode = 1;
}

function fnv1a(value) {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(String(value ?? ""))) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

function readEnglishMessages(projectRoot) {
  const sourcePath = path.join(projectRoot, "src", "locales", "en.js");
  const source = fs.readFileSync(sourcePath, "utf8");
  const start = source.indexOf("{");
  const end = source.lastIndexOf("};");
  if (start < 0 || end < start) {
    throw new Error(`Unable to extract the object from ${sourcePath}`);
  }
  const parsed = JSON.parse(source.slice(start, end + 1));
  if (!parsed.messages || typeof parsed.messages !== "object") {
    throw new Error(`${sourcePath} has no messages object`);
  }
  return parsed.messages;
}

const packArgument = process.argv[2];
if (!packArgument) {
  console.error("Usage: node scripts/validate-language-pack.mjs <language-pack.json>");
  process.exit(2);
}

const projectRoot = process.cwd();
const packPath = path.resolve(packArgument);
const filenameCode = path.basename(packPath, ".json");

if (!/^[A-Za-z]{2,8}(?:-[A-Za-z0-9]{1,8})*$/.test(filenameCode)) {
  fail(`Filename "${path.basename(packPath)}" is not a BCP 47-style language tag.`);
}

let pack;
let english;
try {
  pack = JSON.parse(fs.readFileSync(packPath, "utf8"));
  english = readEnglishMessages(projectRoot);
} catch (error) {
  fail(error.message);
  process.exit();
}

if (!pack.messages || typeof pack.messages !== "object" || Array.isArray(pack.messages)) {
  fail("messages must be a JSON object.");
  process.exit();
}

if (pack.code && pack.code !== filenameCode) {
  fail(`code "${pack.code}" does not match filename code "${filenameCode}".`);
}

const missing = [];
const outdated = [];
const manuallyModified = [];
const removed = [];

for (const [key, sourceText] of Object.entries(english)) {
  if (!(key in pack.messages)) {
    missing.push(key);
    continue;
  }
  const status = pack.message_status?.[key];
  if (status?.source_hash && status.source_hash !== fnv1a(sourceText)) {
    outdated.push(key);
  }
  if (
    status?.translation_hash &&
    status.translation_hash !== fnv1a(pack.messages[key])
  ) {
    manuallyModified.push(key);
  }
}

for (const key of Object.keys(pack.messages)) {
  if (!(key in english)) removed.push(key);
}

console.log(JSON.stringify({
  file: packPath,
  code: filenameCode,
  english_key_count: Object.keys(english).length,
  translated_key_count: Object.keys(pack.messages).length,
  missing_count: missing.length,
  outdated_count: outdated.length,
  manually_modified_count: manuallyModified.length,
  removed_count: removed.length,
  missing,
  outdated,
  manually_modified: manuallyModified,
  removed
}, null, 2));

if (missing.length || outdated.length || removed.length) {
  process.exitCode = 1;
}
