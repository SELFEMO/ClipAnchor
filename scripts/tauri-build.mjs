import { spawnSync } from 'node:child_process';

const targetIndex = process.argv.indexOf('--target');
const targetTriple = targetIndex >= 0 ? process.argv[targetIndex + 1] : '';
const isMacTarget = process.platform === 'darwin' || String(targetTriple).includes('apple-darwin');
const tauriArgs = ['run', 'tauri', '--', 'build'];

if (isMacTarget) {
  // macOS 需要私有 API feature 才能保持无边框透明圆角窗口；其他平台不应继承该 macOS 专属开关。
  // macOS needs the private API feature to preserve frameless transparent rounded windows; other platforms should not inherit this macOS-only switch.
  tauriArgs.push('--features', 'macos-private-api');
}

if (targetTriple) {
  // 显式目标平台用于交叉构建和 release 产物命名，避免脚本误用当前主机架构。
  // The explicit target drives cross-builds and release artifact naming so scripts do not accidentally use the host architecture.
  tauriArgs.push('--target', targetTriple);
}

const result = spawnSync('npm', tauriArgs, {
  stdio: 'inherit',
  shell: process.platform === 'win32'
});

process.exit(result.status ?? 1);
