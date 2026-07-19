<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="ClipAnchor logo" width="128" />
</p>

<h1 align="center">ClipAnchor · 剪贴锚</h1>

<p align="center">
  便携、安静、可置顶的跨平台剪贴板工作台
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">简体中文</a>
</p>

## 项目简介

ClipAnchor 是一个使用 Rust、Tauri 与 React 构建的跨平台剪贴板置顶工具。它会在后台监听文本、图片和文件复制行为，把内容整理成轻量桌面弹窗，并把非敏感内容写入本地历史记录。重要内容可以收藏、再次置顶、复制回剪贴板，也可以在历史列表中快速搜索和管理。

ClipAnchor 的核心目标是 **“便携”** 和 **“安静”**。运行数据默认保存在程序同级 `data/` 目录内，便于备份和迁移；开机自启动时默认进入后台轻量模式，不弹出主界面，只保留托盘图标、剪贴板监听和数据库服务。

> 本项目仍在持续迭代。下载、安装或升级前，建议先阅读发行说明并备份重要数据。

## AI 编程提示

本项目由 AI 辅助编程完成。

正式发布或生产使用前，应由开发者复核代码，在所有目标平台重新测试，并使用自有样本验证剪贴板监听、弹窗、历史记录、自启动、安装器和更新机制等核心行为，同时确认所有第三方二进制文件的许可证合规性。

## 当前验证状态

| 平台 | 状态 | 说明 |
|---|---|---|
| Windows x64 | 已验证 | 已通过桌面启动、托盘、剪贴板、历史记录、更新和命令行基础冒烟测试。 |
| Windows ARM64 | 待验证 | 仍需真实设备安装包与运行验证。 |
| macOS ARM64 | 已验证 | 已通过 Apple Silicon APP/DMG 基础运行验证。 |
| macOS x64 | 待验证 | 仍需 Intel macOS 安装包与运行验证。 |
| Linux x64 | 已验证 | Ubuntu x64 编译、桌面启动、托盘、剪贴板、历史记录、轻量模式自启动和核心设置均已验证。由于 Linux 桌面与 Wayland 接口无法保证跨环境一致性，全局快捷键和弹窗位置设置会在 Linux 中主动隐藏。 |
| Linux ARM64 | 待验证 | 仍需 ARM64 Linux 安装包与运行验证。 |

该表仅作为精简发布检查清单。每次发布前仍应复测长时间后台运行、开机轻量模式、安装器交接和更新分发。

## 功能特性

| 模块 | 能力 |
|---|---|
| 置顶弹窗 | 每次复制生成独立桌面弹窗，支持 Pin、Copy、Unpin、自动销毁、拖动和智能堆叠。 |
| 剪贴板类型 | 支持文本、图片、文件和混合剪贴板内容。 |
| 后台轻量模式 | 开机自启动时默认静默运行，不弹主窗口；可通过托盘恢复界面，受支持平台也可使用快捷键。 |
| 单实例运行 | 重复启动不会保留第二个长期运行进程，而是唤醒并置前已有主窗口。 |
| 历史记录 | 使用 SQLite 本地存储，支持搜索、类型过滤、文本编辑、单条删除、批量删除和历史直接置顶。 |
| 收藏保护 | 收藏内容独立显示，同时保留在普通历史记录中；常规清理时默认保留收藏项。 |
| 隐私过滤 | 支持关闭、轻量、智能三档策略；轻量模式使用本地规则识别常见敏感内容。 |
| 快捷键 | Windows 与 macOS 支持置顶服务、历史服务、主界面、轻量模式和主题等全局操作；Linux 端隐藏该设置。 |
| 数据管理 | 支持导入/导出 JSON 或带完整属性的 CSV 历史记录、按天数清理、查看数据库位置，并管理自动轮转的运行日志。 |
| 外观设置 | 支持深色、浅色、跟随系统、界面缩放、弹窗缩放、瞬态滚动条和动画控制。 |
| 多语言 | 内置英文与简体中文，并支持可增量更新的本地扩展语言包。 |
| 便携化 | 历史记录、设置、资源、导出文件、语言包、更新包和日志均保存在 `data/` 目录中。 |

## 下载与安装

已发布的安装包和便携包可在 [Releases](https://github.com/SELFEMO/ClipAnchor/releases) 页面下载。

1. 打开最新 Release。
2. 下载与操作系统和 CPU 架构匹配的文件。
3. 安装软件或解压便携包。
4. 启动 ClipAnchor，并在设置中配置隐私过滤、外观、语言、数据清理策略，以及受支持平台的快捷键。

若当前平台没有可用安装包，请从源码构建。升级或替换便携版前，建议先备份 `data/` 目录中的重要文件。

## 快速开始

### 环境要求

- Git
- Node.js 与 npm
- Rust 工具链
- Tauri 所需的平台编译依赖
- Windows 开发所需的 Microsoft Visual Studio Build Tools 与 WebView2 Runtime
- macOS 或 Linux 打包所需的对应原生构建环境

### 开发运行

```bash
git clone https://github.com/SELFEMO/ClipAnchor.git
cd ClipAnchor
npm install
npm run desktop:dev
```

必须在克隆后的 `ClipAnchor` 目录内执行 `npm install` 和 `npm run desktop:dev`。若在上级目录执行，会因找不到 `package.json` 出现 `Could not read package.json` 错误。

需要干净重建时，请执行：

```bash
npm run clean
```

项目根目录包含 `.cargo/config.toml`，Cargo 会使用其中配置的 sparse 注册表设置解析 Rust 依赖。

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

当前项目还提供以下目标构建命令：

```bash
npm run desktop:build:windows-x64
npm run desktop:build:macos-arm64
npm run desktop:build:macos-x64
npm run desktop:build:linux-x64
```

在 Ubuntu 上首次构建 Linux x64 包前，请先安装 Tauri 原生依赖：

```bash
sudo apt update
sudo apt install -y build-essential curl wget file libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libwebkit2gtk-4.1-dev
rustup target add x86_64-unknown-linux-gnu
npm run desktop:build:linux-x64
```

#### macOS Apple Silicon 构建

在 macOS 主机上首次构建 Apple Silicon 包前，先添加 Rust ARM64 目标：

```bash
rustup target add aarch64-apple-darwin
npm run desktop:build:macos-arm64
```

#### 构建输出与安装包格式

Tauri 会把安装包输出到 `src-tauri/target/release/bundle/`；指定 target 时会输出到 `src-tauri/target/<target-triple>/release/bundle/`。项目脚本随后会把可分发产物整理到根目录 `release/`。

Linux 构建目标包含 DEB 和 RPM；Windows 包含 NSIS 与 MSI；macOS 包含 APP 与 DMG。macOS DMG 应在 macOS 上生成，公开分发前还应使用维护者自己的 Apple Developer 凭据完成签名和公证。

## Linux 桌面说明

### 调整弹窗位置限制

Linux 端暂不提供“**调整弹窗位置**”设置。Wayland 下顶层窗口位置由桌面合成器统一管理，应用无法可靠强制指定绝对屏幕坐标，因此 ClipAnchor 会在 Linux 中隐藏该设置，避免向用户展示可能无效的选项。弹窗仍保持置顶，并可在 Pin 后手动拖动。

### 全局快捷键限制

Linux 端会隐藏全局快捷键设置。不同发行版与桌面会话的 Wayland 门户、授权策略和 GNOME 快捷键行为不一致，无法保证稳定生效，因此 ClipAnchor 不展示可能无效的入口；相关操作可通过托盘菜单和主界面完成。

### 开机自启动

Linux 自启动项写入 `$XDG_CONFIG_HOME/autostart/clipanchor.desktop`；未设置 `XDG_CONFIG_HOME` 时使用 `~/.config/autostart/clipanchor.desktop`。登录后 ClipAnchor 直接进入轻量模式。

## 使用方法

1. 启动 ClipAnchor，确认“置顶服务”和“历史记录服务”处于开启状态。
2. 复制文本、图片或文件，桌面会出现紧凑弹窗。
3. 点击 **Pin** 后，弹窗保持置顶，并显示 **Copy**、**Unpin** 等操作。
4. 在剪贴板页面中搜索历史记录、收藏重要内容、编辑文本记录，或从历史记录重新生成置顶弹窗。
5. 在设置页面调整主题、语言、缩放、隐私过滤、自动销毁时间和数据清理策略；快捷键仅在受支持平台显示，Linux 同时隐藏快捷键和弹窗位置设置。
6. 开启自启动后，下次登录系统时 ClipAnchor 会进入后台轻量模式；双击托盘图标或点击“显示 ClipAnchor”可恢复主窗口，Windows 与 macOS 也可使用已配置快捷键。

## 扩展语言包

ClipAnchor 内置英文和简体中文，其他语言可通过本地 JSON 扩展语言包加载。

### 翻译参考文件

制作语言包时请参考：

- 英文原始文案与权威键名：[`src/locales/en.js`](src/locales/en.js)
- 公开 JSON 模板：[`docs/i18n/language-pack.template.json`](docs/i18n/language-pack.template.json)
- 详细翻译说明：[`docs/i18n/README.md`](docs/i18n/README.md)
- 可选校验脚本：[`scripts/validate-language-pack.mjs`](scripts/validate-language-pack.mjs)

请复制 `src/locales/en.js` 中 `messages` 对象的键名，只翻译值，不要翻译、删除或随意修改键名。

### 最小兼容结构

```json
{
  "format": "clipanchor-language-pack",
  "code": "fr",
  "label": "French",
  "native_name": "Français",
  "source": "manual",
  "source_locale": "en",
  "messages": {
    "settings": "Paramètres",
    "cancel": "Annuler",
    "ok": "OK"
  },
  "message_status": {}
}
```

以上内容仅用于展示格式，不是完整语言包。对外发布的语言包应包含 `src/locales/en.js` 中当前所有界面键。

语言包要求：

- 文件编码必须为 UTF-8；
- 必须是合法 JSON，不能包含注释或末尾多余逗号；
- `messages` 必须是对象，键和值都应为字符串；
- 必须保留 `{language}`、`{count}`、`{error}`、`{days}` 等占位符；
- 必须保留 `\n`、`\"` 等 JSON 转义含义；
- 不要在语言包中保存 API Key、剪贴板内容或其他私密数据；
- 手工制作的语言包可以先将 `message_status` 留空。

### 文件名与语言代号标准

文件名应采用 IETF BCP 47 风格的语言标签：

| 语言 | 文件名 |
|---|---|
| 日语 | `ja.json` |
| 法语 | `fr.json` |
| 德语 | `de.json` |
| 西班牙语 | `es.json` |
| 巴西葡萄牙语 | `pt-BR.json` |
| 繁体中文（台湾） | `zh-TW.json` |
| 塞尔维亚语（拉丁字母） | `sr-Latn.json` |

命名规则：

- 使用连字符 `-`，不要使用下划线 `_`；
- 主语言通常小写，例如 `fr`、`ja`；
- 书写系统首字母大写，例如 `Latn`、`Hant`；
- 国家或地区通常大写，例如 `BR`、`TW`；
- 不要使用空格或 `French.json` 等显示名称；
- JSON 中的 `code` 应与文件名去掉 `.json` 后一致；
- `auto`、`en`、`en-*`、`zh`、`zh-CN` 和 `zh-Hans*` 属于自动或内置语言范围。

### 安装语言包

将完成的 JSON 文件复制到：

```text
data/locales/
```

返回 ClipAnchor 设置页，点击“打开语言目录”右侧的“刷新语言包”。后端会立即重新扫描当前 `data/locales/`，无需退出托盘进程或重启软件。

### 校验语言包

在项目根目录执行：

```bash
node scripts/validate-language-pack.mjs data/locales/fr.json
```

校验脚本会检查文件名、JSON 结构、缺失项、英文原文变化项、人工修改译文和已删除键。

### 增量更新判断

ClipAnchor 会将扩展语言包与当前英文界面文案进行比较：

| 状态 | 判断方式 | 处理 |
|---|---|---|
| 缺失项 | 英文源文件存在该键，但语言包 `messages` 中不存在 | 加入更新集合。 |
| 原文已变化 | 当前英文文本与已记录的 `source_hash` 不一致 | 标记为过期。 |
| 已删除项 | 语言包存在该键，但当前英文源文件已不存在 | 在本地删除，不调用翻译接口。 |
| 人工修改 | 当前译文与已记录的 `translation_hash` 不一致 | 设置 `modified: true` 并保留人工译文。 |
| 文件损坏 | JSON 无法解析、结构无效或没有可用 `messages` | 标记为损坏，要求修复或重新生成。 |

`source_hash` 和 `translation_hash` 只是用于判断文本是否变化的轻量指纹，不是安全加密哈希。

增量更新遵循以下原则：

1. 只翻译缺失项和英文原文发生变化的项；
2. 未变化译文直接复用；
3. 已从英文源文件删除的键只在本地清理；
4. 检测到人工修改时不自动覆盖；
5. 人工修改项对应的英文原文后来发生变化时，应由译者重新复核语义。

### 常见问题

- **有可用更新**：语言包仍可使用，但存在缺失、变化或已删除的键；
- **文件错误/损坏**：文件不能被安全加载，需要修复或重新生成；
- 语言没有显示时，请确认文件位于 `data/locales/`、扩展名为 `.json`、文件名是有效语言标签，并且 JSON 格式正确；
- 修改文件后点击“刷新语言包”即可重新读取；若文件仍未显示，请检查设置页展示的实际语言目录路径；
- 界面出现英文回退通常表示某些键缺失；
- 占位符显示异常通常表示原始 `{...}` 占位符被修改或删除。

## 数据位置

ClipAnchor 默认把运行数据放在程序同级目录下：

```text
data/
├── clipanchor.db
├── settings.json
├── locales/
├── resources/
├── exports/        # JSON 和 CSV 历史导出均包含记录属性。
├── updates/
└── logs/
    ├── clipanchor.log
    └── clipanchor-*.log
```

日志会在当前文件过大时自动轮转。设置 → 日志管理中提供日志占用、保留天数、打开日志目录、刷新和清理入口。

复制整个安装目录即可迁移历史记录和设置。删除或替换 `data/` 前，请先确认已经备份重要内容。

## 隐私与数据安全

ClipAnchor 会处理用户复制的文本、图片和文件路径。

- 不要在公共设备上长期保留敏感剪贴板内容；
- 根据实际需要设置隐私过滤、内容类型过滤和数据清理规则；
- 提交 Issue 前，应对日志和截图进行脱敏；
- 不要将 `data/`、`.env`、API Key、签名证书、私钥或本机构建凭据提交到 Git；
- 清理本地数据前，请先备份需要保留的记录。

## 更新通道

设置中开启 **自动更新** 后，ClipAnchor 启动时会在后台静默检查 GitHub Releases。启动检查处于检查中、无更新、普通失败或云端版本没有当前平台兼容安装包时，不会自动弹出更新卡片。

只有兼容安装包已经准备完成时才会主动提示。网络或下载失败只在手动检查窗口中展示，不会留下反复触发的启动提示。

用户点击 **检查更新** 后，会立即进入应用内状态卡片，并展示正在检查、正在下载、准备安装、已是最新、没有兼容安装包或失败等状态。发布标签建议使用 `pre-release-v...` 或 `release-v...`。

更新包会自动选择：

- Windows 优先使用 `ClipAnchor_Windows_x64.exe`；若没有 EXE，则选择匹配系统语言的 MSI，例如 `ClipAnchor_Windows_x64_zh-CN.msi` 或 `ClipAnchor_Windows_x64_en-US.msi`；
- macOS 使用 DMG，并根据文件名过滤架构，Apple Silicon 会选择 ARM64 或 universal 包，不会误选 Intel-only 包；
- Linux 根据发行版家族选择 DEB 或 RPM。

若 `data/updates/` 中已有资产 URL 指纹与服务端预期大小均一致的完整安装包，才会直接复用。新包先写入同目录临时文件，校验完成后再重命名，并在新包可用后清理旧安装包。用户点击 **立即更新** 后，后端通过独立平台脚本交接安装，并在支持的平台上于覆盖成功后重启 ClipAnchor。

## 项目结构

| 路径 | 作用 |
|---|---|
| `src/index.html` | 主应用的 Vite 入口。构建后输出 `dist/index.html`，供 Tauri 主窗口和弹窗加载。 |
| `src/` | React 前端，包括主界面、剪贴板页、设置页、弹窗页、接口封装、多语言、Hooks 和全局样式。 |
| `src/locales/` | 内置英文与简体中文界面文案。 |
| `src-tauri/` | Rust/Tauri 后端，包括剪贴板监听、数据库、托盘、自启动、快捷键、单实例、更新和窗口控制。 |
| `data/` | 便携运行数据目录，用于保存数据库、设置、语言包、资源、导出文件、更新包和日志。 |
| `docs/index.html` | 独立官网页面，用于 GitHub Pages 或静态托管，不参与桌面应用运行。 |
| `docs/i18n/` | 可公开发布的扩展语言包说明与 JSON 模板。 |
| `scripts/` | 开发、校验、清理、构建产物收集和便携包打包脚本。 |
| `release/` | 构建完成后存放安装包与便携包的分发目录。 |

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

## 提交问题

提交 Issue 时，建议提供：

- 操作系统及版本；
- ClipAnchor 版本；
- 清晰的复现步骤；
- 期望行为和实际行为；
- 已脱敏的日志；
- 必要的截图或录屏。

请勿公开剪贴板原文、访问令牌、私人路径或其他敏感信息。

## 参与开发

1. Fork 本仓库并创建范围明确的分支；
2. 安装依赖并确认开发模式可以正常启动；
3. 保持英文和简体中文的界面键同步；
4. 修改界面文案时，同时更新两份内置语言文件；
5. 新增或修改英文键后，检查扩展语言包的增量更新行为；
6. 提交前运行项目现有的构建、格式和静态检查；
7. 发起 Pull Request，并说明变更内容和验证方式。

推荐提交信息格式：

```text
feat: add clipboard filter
fix: preserve manually edited translations
docs: improve language-pack guide
```

## 许可证

ClipAnchor 使用 Apache License 2.0 许可证。完整许可证正文见根目录 [`LICENSE`](LICENSE)。
