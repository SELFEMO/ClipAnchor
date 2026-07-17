import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';

const host = '127.0.0.1';
const preferredPort = 1420;
const scanLimit = 160;
const startupTimeoutMs = 30_000;
const isWindows = process.platform === 'win32';
const npmExecPath = process.env.npm_execpath?.trim();
const npmCommand = npmExecPath ? process.execPath : (isWindows ? 'npm.cmd' : 'npm');
const npmPrefixArgs = npmExecPath ? [npmExecPath] : [];
const npmNeedsShell = isWindows && !npmExecPath;
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

  // 固定范围全部被占用或被 Windows 保留时回退到系统分配端口，是为了避免 EACCES 直接终止整个 Tauri 开发流程。
  // Falling back to an OS-assigned port when the preferred range is occupied or reserved prevents EACCES from terminating the whole Tauri development flow.
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
        if (Date.now() >= deadline) {
          reject(new Error(`Timed out while waiting for Vite on ${host}:${port}.`));
        } else {
          setTimeout(attempt, 120);
        }
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
    // npm 在 Windows 上会再创建一层 cmd.exe，按进程树终止才能避免退出 Tauri 后遗留占用端口的 Vite 子进程。
    // npm creates an extra cmd.exe layer on Windows, so terminating the process tree prevents a Vite child from keeping the port after Tauri exits.
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

async function main() {
  const port = await chooseDevPort();
  const devUrl = `http://${host}:${port}`;
  if (port !== preferredPort) {
    console.log(`Port ${preferredPort} is unavailable or reserved; using ${port} instead.`);
  }

  viteProcess = spawn(
    npmCommand,
    [...npmPrefixArgs, 'run', 'dev', '--', '--host', host, '--port', String(port), '--strictPort'],
    {
      stdio: 'inherit',
      // npm 脚本自身提供的 npm_execpath 由 Node 直接执行，是为了绕开 Windows 对 npm.cmd 与 JSON 参数的二次命令行解析。
      // Executing npm's own npm_execpath through Node avoids Windows re-parsing npm.cmd and JSON arguments through a second command shell.
      shell: npmNeedsShell,
      windowsHide: true,
      env: { ...process.env, CLIPANCHOR_DEV_PORT: String(port) }
    }
  );

  viteProcess.once('error', (error) => {
    console.error(`Failed to start Vite: ${error.message}`);
    shutdown(1);
  });

  await waitForServer(port);

  const userArgs = process.argv.slice(2);
  const tauriArgs = [...npmPrefixArgs, 'run', 'tauri', '--', 'dev', ...userArgs];
  if (process.platform === 'darwin') {
    // macOS 透明窗口依赖 macos-private-api；通过 npm 的 `--` 分隔符传参，是为了避免 npm 把 --features 当成自身配置并丢给 Cargo。
    // macOS transparent windows depend on macos-private-api; the npm `--` separator keeps --features from being parsed as npm config and leaking to Cargo args.
    tauriArgs.push('--features', 'macos-private-api');
  }

  // Tauri 的 JSON Merge Patch 会覆盖固定 devUrl 并删除内置 beforeDevCommand，因为 Vite 已由本脚本以通过权限探测的端口启动。
  // Tauri's JSON Merge Patch overrides the fixed devUrl and removes the built-in beforeDevCommand because this script already started Vite on a permission-tested port.
  tauriArgs.push('--config', JSON.stringify({
    build: {
      beforeDevCommand: null,
      devUrl
    }
  }));

  tauriProcess = spawn(npmCommand, tauriArgs, {
    stdio: 'inherit',
    shell: npmNeedsShell,
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

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => shutdown(0));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  shutdown(1);
});
