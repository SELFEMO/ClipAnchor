import { rm } from 'node:fs/promises';
import { join } from 'node:path';

const root = process.cwd();

const targets = [
  'node_modules',
  'package-lock.json',
  join('src-tauri', 'target')
];

for (const target of targets) {
  const path = join(root, target);
  try {
    // 使用 Node 原生递归删除是为了避开 macOS 上 rm 偶发的 “Directory not empty”，同时兼容 Windows shell。
    // Native recursive removal avoids intermittent macOS “Directory not empty” failures and keeps the cleanup command shell-agnostic on Windows.
    await rm(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 120 });
    console.log(`cleaned ${target}`);
  } catch (error) {
    console.warn(`skip ${target}: ${error.message}`);
  }
}

console.log('clean finished');
