import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const configuredPort = Number.parseInt(process.env.CLIPANCHOR_DEV_PORT ?? '1420', 10);
const devPort = Number.isInteger(configuredPort) && configuredPort > 0 ? configuredPort : 1420;

export default defineConfig({
  // 将桌面应用入口放进 src/，是为了保持项目根目录干净，同时让 Vite 仍然从同一个前端源码根加载 React 资源。
  // Keeping the desktop entry inside src/ keeps the project root clean while letting Vite load React assets from one frontend source root.
  root: 'src',
  plugins: [react()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    // 开发脚本探测端口后通过环境变量传入，是为了让配置、Vite 与 Tauri 始终指向同一个可绑定地址。
    // The development launcher passes its probed port through the environment so config, Vite, and Tauri always target the same bindable address.
    port: devPort,
    strictPort: true
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    // 构建产物仍输出到项目根目录 dist/，是为了继续匹配 Tauri frontendDist 和弹窗的 index.html 加载路径。
    // Build output stays in the root dist/ folder so it continues to match Tauri frontendDist and popup index.html loading.
    outDir: '../dist',
    emptyOutDir: true,
    target: 'esnext',
    minify: 'esbuild'
  }
});
