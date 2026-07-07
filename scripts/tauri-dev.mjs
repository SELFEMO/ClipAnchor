import { spawnSync } from 'node:child_process';

const args = ['tauri', 'dev'];

if (process.platform === 'darwin') {
  // macOS 透明窗口依赖 macos-private-api；只在 macOS 透传该 feature，避免 Windows/Linux 构建命令携带无关配置。
  // macOS transparent windows depend on macos-private-api; forwarding it only on macOS keeps Windows/Linux commands free from unrelated flags.
  args.push('--features', 'macos-private-api');
}

const result = spawnSync('npm', ['run', ...args, ...process.argv.slice(2)], {
  stdio: 'inherit',
  shell: process.platform === 'win32'
});

process.exit(result.status ?? 1);
