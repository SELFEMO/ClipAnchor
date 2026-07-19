import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { resolveNodeBinary } from './node-tool.mjs';
import { ensureCargoLock, ensureRustToolchain } from './toolchain.mjs';

function main() {
  ensureRustToolchain();
  ensureCargoLock();
  const targetIndex = process.argv.indexOf('--target');
  const targetTriple = targetIndex >= 0 ? process.argv[targetIndex + 1] : '';
  const normalizedTarget = String(targetTriple).toLowerCase();
  const isMacTarget = process.platform === 'darwin' || normalizedTarget.includes('apple-darwin');
  const isWindowsTarget = process.platform === 'win32' || normalizedTarget.includes('windows');
  const isLinuxTarget = process.platform === 'linux' || normalizedTarget.includes('linux');
  const cargoLockPath = join(process.cwd(), 'src-tauri', 'Cargo.lock');

  if (!existsSync(cargoLockPath)) {
    throw new Error('Missing src-tauri/Cargo.lock. Restore it from Git before building: git restore src-tauri/Cargo.lock');
  }

  const tauriEntry = resolveNodeBinary('@tauri-apps/cli', 'tauri');
  // 构建脚本始终使用 CI 模式，是为了避免 npm run 在无人值守或重定向终端中触发交互提示及相关警告。
  // The build script always uses CI mode so npm run cannot trigger interactive prompts or related warnings in unattended or redirected terminals.
  const tauriArgs = [tauriEntry, 'build', '--ci'];
  const bundleTargets = isWindowsTarget
    ? ['nsis', 'msi']
    : isMacTarget
      ? ['app', 'dmg']
      : isLinuxTarget
        ? ['deb', 'rpm']
        : [];

  if (bundleTargets.length) {
    // 每个平台只请求其支持的安装包，是为了避免 Tauri 在 npm run 构建期间为其他平台目标输出无意义警告。
    // Requesting only the installers supported by the selected platform prevents Tauri from printing irrelevant cross-platform target warnings during npm run builds.
    tauriArgs.push('--bundles', bundleTargets.join(','));
  }

  if (isMacTarget) {
    // macOS 构建只在目标确实为 Darwin 时启用透明窗口 feature，避免其他平台继承无关参数和构建噪声。
    // The transparent-window feature is enabled only for Darwin targets so other platforms do not inherit irrelevant arguments or build noise.
    tauriArgs.push('--features', 'macos-private-api');
  }

  if (targetTriple) {
    // 显式目标用于交叉构建和产物命名，避免脚本错误地使用当前主机架构。
    // The explicit target drives cross-builds and artifact naming so the script does not accidentally use the host architecture.
    tauriArgs.push('--target', targetTriple);
  }

  const result = spawnSync(process.execPath, tauriArgs, {
    stdio: 'inherit',
    windowsHide: true
  });

  if (result.error) {
    console.error(`Failed to start the Tauri build: ${result.error.message}`);
    process.exit(1);
  }
  process.exit(result.status ?? 1);
}

try {
  main();
} catch (error) {
  // 构建前置错误只输出一条可执行信息，是为了避免 Node 堆栈掩盖真正缺失的工具或文件。
  // Build preflight failures print one actionable line so a Node stack trace cannot obscure the missing tool or file.
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
