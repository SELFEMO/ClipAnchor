import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

function probe(command) {
  return spawnSync(command, ['--version'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true
  });
}

export function ensureRustToolchain() {
  for (const command of ['cargo', 'rustc']) {
    const result = probe(command);
    if (!result.error && result.status === 0) continue;

    const detail = result.error?.code === 'ENOENT'
      ? `${command} was not found in PATH`
      : (result.stderr || result.stdout || result.error?.message || 'unknown error').trim();

    // 在启动 Vite 前校验 Rust 工具链，是为了避免 Tauri 失败后留下无用的开发服务器，并把重复底层错误收敛为一条可执行提示。
    // Checking the Rust toolchain before Vite starts prevents a useless dev server from surviving a Tauri failure and collapses duplicate low-level errors into one actionable message.
    throw new Error(`Rust toolchain is unavailable: ${detail}. Install Rust with rustup, restart the terminal, and verify both cargo and rustc are on PATH.`);
  }
}


export function ensureCargoLock() {
  const manifestPath = join(process.cwd(), 'src-tauri', 'Cargo.toml');
  const lockPath = join(process.cwd(), 'src-tauri', 'Cargo.lock');
  if (!existsSync(manifestPath)) throw new Error('Missing src-tauri/Cargo.toml.');

  if (existsSync(lockPath)) {
    // --no-deps 仍由 --locked 判定锁是否过期，但不再输出约 3MB 的依赖 JSON。
    // 丢弃 stdout 是因为 Node spawnSync 默认只缓冲 1MB，完整 metadata 会把一次成功检查变成失败。
    // --no-deps still lets --locked decide whether the lock is stale, without emitting the ~3MB dependency JSON.
    // Stdout is discarded because Node spawnSync buffers only 1MB by default, so the full metadata document turns a successful check into a failure.
    const metadata = spawnSync('cargo', [
      'metadata',
      '--manifest-path', manifestPath,
      '--format-version', '1',
      '--locked',
      '--no-deps',
      '--quiet'
    ], {
      encoding: 'utf8',
      stdio: ['ignore', 'ignore', 'pipe'],
      windowsHide: true
    });

    if (!metadata.error && metadata.status === 0) return;

    const detail = (metadata.stderr || metadata.error?.message || 'unknown error').trim().slice(0, 2000);
    // 已有锁文件时拒绝 generate-lockfile，避免一次检查失败就重解整份依赖图。
    // An existing lock must not be regenerated, so a failed check cannot rewrite the dependency graph.
    throw new Error(`Cargo.lock is out of date. Restore src-tauri/Cargo.lock instead of regenerating it: ${detail}`);
  }

  // 只有锁文件缺失时才生成，覆盖压缩包未带锁的情况；清单里的波浪号版本把 Tauri 限制在当前次版本内。
  // Generate a lock only when it is missing, such as an archive shipped without one. Tilde requirements in the manifest keep Tauri on the current minor.
  const generated = spawnSync('cargo', [
    'generate-lockfile',
    '--manifest-path', manifestPath,
    '--quiet'
  ], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true
  });

  if (!generated.error && generated.status === 0 && existsSync(lockPath)) return;
  const detail = (generated.stderr || generated.stdout || generated.error?.message || 'unknown error').trim();
  throw new Error(`Cargo.lock could not be synchronized: ${detail}`);
}

const LINUX_PKG_CONFIG_MODULES = [
  'glib-2.0',
  'gobject-2.0',
  'gtk+-3.0',
  'webkit2gtk-4.1',
  'ayatana-appindicator3-0.1',
  'librsvg-2.0'
];

export function ensureLinuxBuildDependencies() {
  if (process.platform !== 'linux') return;

  const missing = [];
  for (const moduleName of LINUX_PKG_CONFIG_MODULES) {
    const result = spawnSync('pkg-config', ['--exists', moduleName], {
      stdio: 'ignore',
      windowsHide: true
    });
    if (result.error?.code === 'ENOENT') {
      throw new Error('pkg-config was not found in PATH. Install it with: sudo apt install -y pkg-config');
    }
    if (result.status !== 0) missing.push(moduleName);
  }
  if (missing.length === 0) return;

  // 在 cargo 编译前检查这些 .pc，是为了避免缺库时先编到一半，再被 gobject-sys 的 pkg-config 输出淹没。
  // Checking these .pc files before cargo compiles avoids a mid-build failure buried in gobject-sys pkg-config output.
  throw new Error(
    `Linux build dependencies are missing (${missing.join(', ')}). `
    + 'Install them with: sudo apt update; sudo apt install -y build-essential curl wget file libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libwebkit2gtk-4.1-dev'
  );
}
