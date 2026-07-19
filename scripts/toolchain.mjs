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

  const metadata = spawnSync('cargo', [
    'metadata',
    '--manifest-path', manifestPath,
    '--format-version', '1',
    '--locked',
    '--quiet'
  ], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true
  });

  if (metadata.status === 0 && existsSync(lockPath)) return;

  // 压缩包中的清单可能比锁文件更新；在启动前静默重建锁文件，可避免 cargo run 先输出索引更新和依赖锁定噪声，同时保证后续构建使用完整依赖图。
  // An archive can contain a manifest newer than its lockfile; quietly regenerating the lock before startup avoids index/locking noise from cargo run and ensures subsequent builds use a complete dependency graph.
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
  const detail = (generated.stderr || generated.stdout || generated.error?.message || metadata.stderr || 'unknown error').trim();
  throw new Error(`Cargo.lock could not be synchronized: ${detail}`);
}
