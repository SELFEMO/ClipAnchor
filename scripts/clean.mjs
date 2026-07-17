import { rm } from 'node:fs/promises';
import { join } from 'node:path';

const root = process.cwd();
const targets = [
  'node_modules',
  join('src-tauri', 'target')
];

for (const target of targets) {
  const path = join(root, target);
  try {
    // 只删除可再生成的构建目录。package-lock.json 与 Cargo.lock 必须保留，
    // 否则重新安装或构建会解析出不同依赖并让 Git 工作区无故变脏。
    // Remove only regenerable build directories. Keep package-lock.json and Cargo.lock
    // so reinstalling or building cannot silently resolve a different dependency graph.
    await rm(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 120 });
    console.log(`cleaned ${target}`);
  } catch (error) {
    console.warn(`skip ${target}: ${error.message}`);
  }
}

console.log('preserved package-lock.json');
console.log('preserved src-tauri/Cargo.lock');
console.log('clean finished');
