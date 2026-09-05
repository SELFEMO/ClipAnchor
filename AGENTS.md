# ClipAnchor agent contract

仅在本仓库使用（根目录同时有 `package.json` 和 `src-tauri/`）。命令必须在仓库根目录执行。
Use this file only in this repo (root contains both `package.json` and `src-tauri/`). Commands must run from the repository root.

本机默认 shell 是 PowerShell。提交信息和 PR 正文用 PowerShell here-string，不要用 bash HEREDOC。
The default shell is PowerShell. Pass commit messages and PR bodies with here-strings, not bash HEREDOC.

本地 Cursor skill 在 `.cursor/skills/clipanchor-dev/`，该目录被 gitignore，**仓库内以本文件为准**。
A local Cursor skill may exist under `.cursor/skills/clipanchor-dev/`; that directory is gitignored. **This file is the source of truth in the repository.**

## Execution gate

不要执行 `run` / `build` 或任何会改 Git 状态的命令，除非用户明确点了指令名：`run`、`build`、`git-status`、`git-update`、`git-commit`、`git-push`、`git-branch`、`git-pr`，或中文等价说法（运行、构建、提交、推送、新分支、开 PR）。
Do not execute `run`, `build`, or any Git-mutating command unless the user explicitly named that instruction (`run`, `build`, `git-status`, `git-update`, `git-commit`, `git-push`, `git-branch`, `git-pr`) or an equivalent Chinese phrase.

写 plan、改完代码、收尾、想“验证一下应用”，都**不是**执行许可。
Writing a plan, finishing edits, wrapping up, or wanting to “verify the app” is **not** permission to run these commands.

## Plan requirement

当用户提到这些指令时，plan 必须包含下面这一节（原文抄写，不要改写成 `npm run build`）：
When the user mentioned these instructions, the plan MUST include this section (copy verbatim; never rewrite as `npm run build`):

```markdown
## ClipAnchor 一键指令
仅当用户随后发出对应指令时执行，不要在改代码后自动跑。
- run: npm run clean; npm install --registry=https://registry.npmmirror.com; npm run desktop:dev
- build: npm run clean; npm install --registry=https://registry.npmmirror.com; npm run desktop:build
```

## Quick reference

| Instruction | Action |
|---|---|
| `run` | Full clean + npmmirror install + `desktop:dev` (long-running) |
| `build` | Full clean + npmmirror install + `desktop:build` (host desktop bundles → `release/`) |
| `git-status` | Read-only `git status` + `git diff` + recent `git log` |
| `git-update` | `git fetch` then `git pull` if upstream exists; no rebase unless asked |
| `git-commit` | Only when the user asks to commit |
| `git-push` | Only when the user asks to push; no force, no skipped hooks |
| `git-branch` | Only when the user asks for a new branch: `git switch -c <name>` |
| `git-pr` | Only when the user asks for a PR: push if needed, then `gh pr create` |

## `run`

为什么用完整 clean + 镜像 install：`npm run clean` 会删掉 `node_modules` 和 `src-tauri/target` 但保留 lockfile，这样每次启动都从已知依赖图重建；`.npmrc` 没有 registry，必须显式走 npmmirror。
Why the full clean + mirrored install: `npm run clean` deletes `node_modules` and `src-tauri/target` while keeping lockfiles, so each start rebuilds from a known graph. `.npmrc` does not set a registry, so npmmirror must be passed explicitly.

为什么必须是 `desktop:dev` 而不是 `npm run dev`：`desktop:dev` 拉起 Vite + Tauri 桌面壳；单独 `npm run dev` 只有网页前端。
Why `desktop:dev` and not `npm run dev`: `desktop:dev` starts Vite plus the Tauri desktop shell. `npm run dev` is the web frontend only.

在仓库根目录执行**这一条**（不要改成 `&&`，不要省略 clean/install）：
Run **this exact chain** from the repo root (do not switch to `&&`, do not skip clean/install):

```powershell
npm run clean; npm install --registry=https://registry.npmmirror.com; npm run desktop:dev
```

若本工作区已有 ClipAnchor 的 `desktop:dev` / Vite / Tauri 在跑，先停掉那一个终端任务。不要 `taskkill /IM node.exe`。
If this workspace already has ClipAnchor `desktop:dev` / Vite / Tauri running, stop that terminal job first. Do not `taskkill /IM node.exe`.

`desktop:dev` 是长跑进程：后台启动，不要等到它退出。
`desktop:dev` is long-running: start it in the background and do not wait for it to exit.

## `build`

为什么不能写成 `npm run build`：仓库里 `npm run build` 只是 Vite 前端构建。桌面安装包入口是 `desktop:build`，产物收到根目录 `release/`。
Why this must not be `npm run build`: in this repo `npm run build` is only the Vite frontend build. The desktop packager is `desktop:build`; artifacts land in root `release/`.

```powershell
npm run clean; npm install --registry=https://registry.npmmirror.com; npm run desktop:build
```

`build` 会结束，但 clean 之后是全量 Rust 编译，前台等待并设足够超时。这是当前主机的桌面包，不要擅自改跑带 target 的脚本，除非用户点名。
`build` does terminate, but a post-clean Rust compile can take a long time; wait in the foreground with a large timeout. This is a host-platform bundle. Do not switch to target-specific scripts unless the user named that target.

## Git

- `git-status`：只读。并行 `git status`、`git diff`、`git log -8 --oneline`。不要 stage / commit / pull。
- `git-update`：先 `git fetch`；有上游再 `git pull`（不要 `--rebase`，除非用户要求）。不要改 git config。
- `git-commit`：仅用户明确要求时。不要 `--no-verify`、不要 force、不要提交 `.env`、证书、`ClipAnchor.md`、`data/`、`release/`。PowerShell：`git commit -m @" ... "@`。
- `git-push`：仅用户明确要求时。禁止 `--force`、`--force-with-lease`、`--no-verify`。
- `git-branch`：仅用户明确要求时。`git switch -c <name>`，不要改 git config。
- `git-pr`：仅用户明确要求时。用 `gh pr create`，PowerShell here-string 传 body；根据该分支全部提交写摘要；完成后给出 PR URL。

## Common mistakes

| Mistake | Reality |
|---|---|
| 改完代码自动 `run` | 没有点名指令就禁止执行 |
| 把指令 `build` 当成 `npm run build` | 必须是 `desktop:build` 那条完整链 |
| 省略 `clean` 或 npmmirror | 每次 `run`/`build` 都保留二者 |
| 提交 `.cursor/` 或 `.vscode/` | 编辑器/AI 目录已 gitignore，不要上传 GitHub |
| 主动 commit / push / 开 PR | 只有用户明确要求时才做 |
