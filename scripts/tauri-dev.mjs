import { spawnSync } from 'node:child_process';

const tauriArgs = ['run', 'tauri', '--', 'dev'];

if (process.platform === 'darwin') {
  // macOS 透明窗口依赖 macos-private-api；通过 npm 的 `--` 分隔符传参，是为了避免 npm 把 --features 当成自身配置并丢给 Cargo。
  // macOS transparent windows depend on macos-private-api; the npm `--` separator keeps --features from being parsed as npm config and leaking to Cargo args.
  tauriArgs.push('--features', 'macos-private-api');
}

const result = spawnSync('npm', [...tauriArgs, ...process.argv.slice(2)], {
  stdio: 'inherit',
  shell: process.platform === 'win32'
});

process.exit(result.status ?? 1);
