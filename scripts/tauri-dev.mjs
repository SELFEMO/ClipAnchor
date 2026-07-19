import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, stat, utimes, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { resolveNodeBinary } from './node-tool.mjs';
import { ensureCargoLock, ensureRustToolchain } from './toolchain.mjs';

const host = '127.0.0.1';
const preferredPort = 1420;
const scanLimit = 160;
const startupTimeoutMs = 30_000;
const isWindows = process.platform === 'win32';
const viteEntry = resolveNodeBinary('vite', 'vite');
const tauriEntry = resolveNodeBinary('@tauri-apps/cli', 'tauri');
let viteProcess = null;
let tauriProcess = null;
let shuttingDown = false;

function tryListen(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.unref();
    server.once('error', () => resolve(null));
    server.listen({ host, port, exclusive: true }, () => {
      const address = server.address();
      const selectedPort = typeof address === 'object' && address ? address.port : null;
      server.close(() => resolve(selectedPort));
    });
  });
}

async function chooseDevPort() {
  for (let offset = 0; offset < scanLimit; offset += 1) {
    const selected = await tryListen(preferredPort + offset);
    if (selected) return selected;
  }

  // 固定范围被占用或被系统保留时回退到动态端口，是为了让开发启动不因单个端口权限问题失败。
  // Falling back to an OS-assigned port when the preferred range is occupied or reserved prevents one port permission issue from aborting development startup.
  const selected = await tryListen(0);
  if (selected) return selected;
  throw new Error('Unable to find an available local port for the Vite development server.');
}

function waitForServer(port) {
  const deadline = Date.now() + startupTimeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      if (viteProcess?.exitCode !== null) {
        reject(new Error(`Vite exited before opening ${host}:${port}.`));
        return;
      }
      const socket = net.createConnection({ host, port });
      socket.setTimeout(800);
      socket.once('connect', () => {
        socket.destroy();
        resolve();
      });
      const retry = () => {
        socket.destroy();
        if (Date.now() >= deadline) reject(new Error(`Timed out while waiting for Vite on ${host}:${port}.`));
        else setTimeout(attempt, 120);
      };
      socket.once('error', retry);
      socket.once('timeout', retry);
    };
    attempt();
  });
}

function stopProcessTree(child) {
  if (!child || child.exitCode !== null || child.killed) return;
  if (isWindows) {
    // Windows 子工具可能继续派生进程，按进程树结束可避免 Tauri 退出后 Vite 仍占用开发端口。
    // Windows tools may spawn descendants, so terminating the process tree prevents Vite from keeping the development port after Tauri exits.
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
  } else {
    child.kill('SIGTERM');
  }
}

function shutdown(exitCode = 0) {
  if (shuttingDown) return;
  shuttingDown = true;
  stopProcessTree(tauriProcess);
  stopProcessTree(viteProcess);
  process.exit(exitCode);
}

async function collectRustSources(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collectRustSources(path));
    else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(path);
  }
  return files.sort();
}

async function ensureRustSourcesTriggerRebuild() {
  const sourceRoot = join(process.cwd(), 'src-tauri', 'src');
  const markerDirectory = join(process.cwd(), 'src-tauri', 'target');
  const markerPath = join(markerDirectory, '.clipanchor-rust-source-hash');
  const sources = await collectRustSources(sourceRoot);
  const digest = createHash('sha256');

  for (const source of sources) {
    digest.update(relative(sourceRoot, source));
    digest.update('\0');
    digest.update(await readFile(source));
    digest.update('\0');
  }

  const currentHash = digest.digest('hex');
  const previousHash = await readFile(markerPath, 'utf8').catch(() => '');
  if (previousHash.trim() === currentHash) return;

  const targetExists = await stat(markerDirectory).then(() => true).catch(() => false);
  if (!targetExists && !previousHash.trim()) {
    // 完整清理后 Cargo 本就会重新编译；只写入内容标记而不额外刷新时间或打印提示，可让首次 npm run 输出保持聚焦。
    // Cargo already rebuilds after a full clean; writing only the content marker avoids redundant timestamp changes and keeps first-run npm output focused.
    await mkdir(markerDirectory, { recursive: true });
    await writeFile(markerPath, `${currentHash}\n`, 'utf8');
    return;
  }

  // 覆盖压缩包可能保留早于现有 target 产物的文件时间；内容哈希变化时刷新 Rust 源文件时间，确保 Cargo 不会误用旧后端。
  // An overwritten archive can preserve timestamps older than the existing target artifacts; refreshing Rust source times when the content hash changes prevents Cargo from reusing a stale backend.
  const now = new Date();
  await Promise.all(sources.map(async (source) => {
    const metadata = await stat(source);
    await utimes(source, metadata.atime, now);
  }));
  await mkdir(markerDirectory, { recursive: true });
  await writeFile(markerPath, `${currentHash}\n`, 'utf8');
  console.log('Rust source content changed; backend rebuild has been forced.');
}

async function main() {
  const forwardedArgs = process.argv.slice(2);
  if (forwardedArgs.some((argument) => ['--help', '-h', '--version', '-V'].includes(argument))) {
    // 帮助和 CLI 版本查询不需要启动前端或 Rust 编译器，直接交给 Tauri 可避免无意义的 Vite 输出与端口占用。
    // Help and CLI version queries need neither the frontend nor the Rust compiler; delegating directly to Tauri avoids irrelevant Vite output and port usage.
    const result = spawnSync(process.execPath, [tauriEntry, 'dev', ...forwardedArgs], {
      stdio: 'inherit',
      windowsHide: true
    });
    if (result.error) throw new Error(`Failed to start the Tauri CLI: ${result.error.message}`);
    process.exit(result.status ?? 1);
  }

  ensureRustToolchain();
  ensureCargoLock();
  await ensureRustSourcesTriggerRebuild();
  const port = await chooseDevPort();
  const devUrl = `http://${host}:${port}`;
  if (port !== preferredPort) console.log(`Port ${preferredPort} is unavailable or reserved; using ${port} instead.`);

  viteProcess = spawn(process.execPath, [viteEntry, '--host', host, '--port', String(port), '--strictPort'], {
    stdio: 'inherit',
    windowsHide: true,
    env: { ...process.env, CLIPANCHOR_DEV_PORT: String(port) }
  });
  viteProcess.once('error', (error) => {
    console.error(`Failed to start Vite: ${error.message}`);
    shutdown(1);
  });

  await waitForServer(port);

  const tauriArgs = [tauriEntry, 'dev', ...process.argv.slice(2)];
  if (process.platform === 'darwin') {
    // macOS 透明窗口需要专属 feature；直接传给 Tauri CLI 可避免 npm 把 --features 误报成未知配置。
    // macOS transparent windows require a dedicated feature; passing it directly to the Tauri CLI prevents npm from warning that --features is an unknown configuration key.
    tauriArgs.push('--features', 'macos-private-api');
  }
  tauriArgs.push('--config', JSON.stringify({ build: { beforeDevCommand: null, devUrl } }));

  tauriProcess = spawn(process.execPath, tauriArgs, {
    stdio: 'inherit',
    windowsHide: true,
    env: { ...process.env, CLIPANCHOR_DEV_PORT: String(port) }
  });
  tauriProcess.once('error', (error) => {
    console.error(`Failed to start Tauri: ${error.message}`);
    shutdown(1);
  });
  tauriProcess.once('exit', (code, signal) => {
    stopProcessTree(viteProcess);
    if (signal) process.kill(process.pid, signal);
    else shutdown(code ?? 1);
  });
}

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => shutdown(0));

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  shutdown(1);
});
