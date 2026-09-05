import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

export function readCargoPackageVersion(root = repoRoot) {
  const text = readFileSync(join(root, 'src-tauri', 'Cargo.toml'), 'utf8');
  let inPackage = false;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.startsWith('[')) {
      inPackage = line === '[package]';
      continue;
    }
    if (!inPackage || !line.startsWith('version')) continue;
    const matched = line.match(/^version\s*=\s*"([^"]+)"/);
    if (matched) return matched[1];
  }
  throw new Error('Could not read [package].version from src-tauri/Cargo.toml.');
}

export function syncNpmPackageVersion(root = repoRoot) {
  const version = readCargoPackageVersion(root);
  const packagePath = join(root, 'package.json');
  const lockPath = join(root, 'package-lock.json');
  let changed = false;

  changed = writeNamedPackageVersion(packagePath, version) || changed;
  changed = writeNamedPackageVersion(lockPath, version) || changed;

  return { version, changed };
}

function writeNamedPackageVersion(path, version) {
  const original = readFileSync(path, 'utf8');
  const next = original.replace(/("name": "clipanchor",\s*"version": ")([^"]+)(")/g, `$1${version}$3`);
  if (next === original) return false;
  if (!next.includes(`"version": "${version}"`)) {
    throw new Error(`Could not update version in ${path}.`);
  }
  writeFileSync(path, next);
  return true;
}

const invokedDirectly = Boolean(process.argv[1]) && import.meta.url === pathToFileURL(process.argv[1]).href;
if (invokedDirectly) {
  try {
    const result = syncNpmPackageVersion();
    if (result.changed) {
      console.log(`Synced npm package version to ${result.version} from Cargo.toml.`);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
