<div align="center">
  <img src="src-tauri/icons/128x128.png" width="92" height="92" alt="ClipAnchor logo" />
  <h1>ClipAnchor · 剪贴锚</h1>
  <p><strong>便携、安静、可置顶的跨平台剪贴板工作台</strong></p>
  <p><a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a></p>
</div>

## 项目简介

ClipAnchor 是一个使用 Rust、Tauri 与 React 构建的跨平台剪贴板置顶工具。它会在后台监听文本、图片和文件复制行为，把内容整理成轻量桌面弹窗，并把非敏感内容写入本地历史记录。重要内容可以收藏、再次置顶、复制回剪贴板，也可以在历史列表中快速搜索和管理。

ClipAnchor 的核心目标是“便携”和“安静”：数据默认保存在程序同级 `data/` 目录内，便于备份和迁移；开机自启动时默认进入后台轻量模式，不弹出主界面，只保留托盘图标、剪贴板监听和数据库服务。

## AI 编程提示

本项目由 AI 辅助编程完成。正式发布或生产使用前，应由开发者复核代码，在所有目标平台重新测试，并使用自有样本验证剪贴板监听、弹窗、历史记录、自启动、安装器和更新机制等核心行为，同时确认所有第三方二进制文件的许可证合规性。

## 当前验证状态

| 平台           | 当前状态 | 说明 |
| ------------- | ------- | ---- |
| Windows x64   | 已验证   | 当前 Windows x64 构建已完成基础冒烟验证，范围包括桌面端启动、托盘恢复、单实例唤醒、设置持久化、剪贴板捕获与历史记录、手动检查更新、通过降版本号验证 GitHub Release 检测与安装包下载、检查更新前清理旧更新包、后台更新下载不再弹出命令行窗口以及 `--version` 参数。已针对关闭到托盘后监听停止和长时间后台空闲问题增加保活修复与看门狗。正式公开分发前，仍需继续复测长时间后台运行、开机轻量模式更新、安装包签名、本地化 MSI 安装界面以及 GitHub Release 资产上传与命名。 |
| Windows ARM64 | 未验证   | 尚未完成原生 ARM64 安装包和运行行为验证。发布前应在真实 Windows ARM64 设备上检查启动、托盘、自启动、全局快捷键、剪贴板权限以及更新资产匹配逻辑。 |
| macOS ARM64   | 未验证   | Apple Silicon 设备上的构建和运行验证仍待完成。发布前应验证 APP/DMG 包、代码签名与公证、剪贴板权限、托盘/菜单行为、自启动 plist 以及更新安装交接流程。 |
| macOS x64     | 未验证   | Intel macOS 的安装包和运行验证仍待完成。发布前应在 Intel 设备或可靠的 Intel 环境中复核 APP/DMG、签名与公证、权限、托盘/菜单、自启动以及更新流程。 |
| Linux x64     | 未验证   | 尚未在不同 Linux 桌面环境下完成 DEB/RPM 安装和运行验证。发布前应测试 desktop entry、托盘支持、自启动、全局快捷键、X11/Wayland 剪贴板行为以及 DEB/RPM 更新包选择。 |
| Linux ARM64   | 未验证   | 尚未完成原生 ARM64 Linux 安装包和运行验证。发布前应在真实 ARM64 设备上测试 DEB/RPM 打包、桌面集成、托盘可用性、剪贴板访问以及更新资产匹配。 |

上述状态是面向打包与运行体验的实用检查清单，不等同于安全审计或长期稳定性保证。**已验证**表示该平台已经通过当前范围的冒烟测试；**未验证**表示代码设计上支持该平台，但仍需要在真实目标环境中完成安装包和运行验证后再发布。每次调整剪贴板轮询、轻量模式、自启动、Tauri、Rust、Node、打包工具链、签名流程或 GitHub Release 更新管线后，都应重新执行长时间后台运行和安装器交接验证。

## 项目结构

| 路径 | 作用 |
|---|---|
| `src/index.html` | 主应用的 Vite 入口。源码入口放在 `src/` 内，构建后仍会输出为 `dist/index.html`，供 Tauri 主窗口和弹窗加载。 |
| `src/` | React 前端，包括主界面、剪贴板页、设置页、弹窗页、接口封装和全局样式。 |
| `src-tauri/` | Rust/Tauri 后端，包括剪贴板监听、数据库、托盘、自启动、快捷键、单实例和窗口控制。 |
| `data/` | 便携数据目录，用于保存数据库、设置、资源、导出文件和日志。 |
| `docs/index.html` | 独立官网页面，用于 GitHub Pages 或静态托管，不参与桌面应用运行。 |
| `scripts/` | 构建产物收集和便携包打包脚本。 |
| `release/` | 构建完成后复制安装包和便携包的分发目录。 |

## 功能特性

| 模块 | 能力 |
|---|---|
| 置顶弹窗 | 每次复制生成独立桌面弹窗，支持 Pin、Copy、Unpin、自动销毁、拖动和堆叠偏移。 |
| 后台轻量模式 | 开机自启动时默认静默运行，不弹主窗口；可通过托盘或快捷键恢复界面。 |
| 单实例运行 | 重复启动不会保留第二个运行进程，而是唤醒并置前已有主窗口。 |
| 历史记录 | 使用 SQLite 本地存储，支持搜索、类型过滤、单条删除、批量删除和历史直接置顶。 |
| 收藏保护 | 收藏内容独立显示，同时保留在普通历史记录中；常规清理时默认保留收藏项。 |
| 隐私过滤 | 支持关闭、轻量、智能三档策略；轻量模式使用本地规则识别常见敏感内容。 |
| 快捷键 | 支持置顶服务、历史服务、显示/隐藏主界面、轻量模式和暂停监听等全局操作。 |
| 数据管理 | 支持导入/导出 JSON 或带完整属性的 CSV 历史记录、按天数清理、查看数据库位置，并管理自动轮转的运行日志。 |
| 外观设置 | 支持深色、浅色、跟随系统、界面缩放、弹窗缩放和动画模式。 |
| 便携化 | 历史记录、设置、资源、导出文件和日志都保存在 `data/` 目录中。 |

## 快速开始

### 开发运行

```bash
npm install --registry=https://registry.npmmirror.com
npm run desktop:dev
```

Windows 开发环境建议先安装 Rust、Node.js、Microsoft Visual Studio Build Tools 和 WebView2 Runtime。项目根目录包含 `.cargo/config.toml`，Cargo 默认使用 sparse 镜像源，适合网络访问 crates.io 不稳定的环境。


### 查看已安装版本

```powershell
clipanchor.exe --version
clipanchor.exe -V
```

macOS 和 Linux 使用安装后的 `clipanchor` 可执行文件并传入相同参数。该命令只输出软件版本并立即退出，不会打开主窗口，也不会启动剪贴板监听服务。

### 构建安装包

```bash
npm run desktop:build
```

Tauri 会把安装包输出到 `src-tauri/target/release/bundle/`。项目脚本会把可分发产物复制到根目录 `release/`，便于查找和发布。

Linux 构建目标为 DEB 和 RPM。Windows 构建目标包含 NSIS 安装器和 MSI 安装包。macOS 构建目标包含 APP 和 DMG。

## 使用方法

1. 启动 ClipAnchor，确认“置顶服务”和“历史记录服务”处于开启状态。
2. 复制文本、图片或文件，桌面会出现紧凑弹窗。
3. 点击 Pin 后，弹窗保持置顶，并显示 Copy、Unpin 等操作。
4. 在剪贴板页面中搜索历史记录，点击星标收藏，点击 Pin 图标可从历史记录生成置顶弹窗。
5. 在设置页面调整主题、语言、缩放、快捷键、弹窗位置、隐私过滤和数据清理策略。
6. 开启自启动后，下次登录系统时 ClipAnchor 会进入后台轻量模式；双击托盘图标、点击“显示 ClipAnchor”或按 `Ctrl+Shift+X` 可恢复主窗口。

## 数据位置

ClipAnchor 默认把运行数据放在程序同级目录下：

```text
data/
├── clipanchor.db
├── settings.json
├── resources/
├── exports/        # JSON 和 CSV 历史导出均包含记录属性。
└── logs/
    ├── clipanchor.log
    └── clipanchor-*.log
```

日志会在当前文件过大时自动轮转。设置 → 日志管理中提供日志占用、保留天数、打开日志目录、刷新和清理入口，归档日志默认保留 7 天。

复制整个项目或安装目录即可迁移历史记录和设置。删除 `data/` 前请先确认已经备份重要内容。

## 更新通道

ClipAnchor 启动时会在后台静默检查 GitHub Releases，主界面关闭时不会弹窗打扰。用户点击 **检查更新** 按钮后会立即进入应用内状态卡片，并展示正在检查、正在下载、准备安装、已是最新、没有兼容安装包或失败等状态。发布标签建议使用 `pre-release-v...` 或 `release-v...`。

更新包会自动选择：Windows 优先使用 `ClipAnchor_Windows_x64.exe`；如果没有 EXE，则选择匹配系统语言的 MSI，例如 `ClipAnchor_Windows_x64_zh-CN.msi` 或 `ClipAnchor_Windows_x64_en-US.msi`。macOS 使用 DMG，Linux 根据发行版家族选择 DEB 或 RPM。每次重新检查前会先清理 `data/updates/` 中的旧安装包，避免多个历史版本残留导致误用。下载完成的新安装包存放在 `data/updates/`，用户点击 **立即更新** 后由系统安装器打开。普通手动启动时会在后台检查更新，若发现新版本则自动弹出更新卡片；开机自启动的轻量模式会保持静默检查和下载，直到用户打开主界面后再提示。

## 构建产物命名

发布脚本会尽量把安装包整理为以下格式：

```text
ClipAnchor_Windows_x64.msi
ClipAnchor_Windows_x64.exe
ClipAnchor_macOS_arm64.dmg
ClipAnchor_Linux_x64.deb
ClipAnchor_Linux_x64.rpm
```

实际输出取决于当前操作系统、CPU 架构和已安装的 Tauri 打包工具链。

## 许可证

ClipAnchor 使用 Apache License 2.0 许可证。完整许可证正文见根目录 `LICENSE`。
