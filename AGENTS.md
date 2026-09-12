# AGENTS.md — 给接手本项目的 coding agent

## 项目是什么

Windows 桌面客户端（Tauri 2 + React + xterm.js），把远端 Linux 服务器上跑在 `tmux` 里的交互式 CLI agent（Hermes Agent、DeepSeek Harness）的终端**原样镜像**到本地窗口；断网/重开后自动附身回同一 tmux 会话。

**必读（按序）**：

1. `README.md` — 用户视角的功能说明与架构图
2. `docs/superpowers/specs/2026-09-11-agent-session-client-design.md` — 设计文档（目标/非目标/架构决策）
3. `docs/superpowers/plans/2026-09-11-agent-session-client.md` — 10 个实现任务（已全部完成，留作参考）

## 架构不变量（改动前先确认没破坏它们）

- **`core` crate 不依赖 Tauri/GUI** — 所有逻辑可在 Linux 上 `cargo test` 验证；`src-tauri` 只做接线 + 事件转发；前端只渲染字节流。
- **客户端不缓存 agent 输出，服务端 tmux 是唯一事实源** — 不解析输出、不做聊天 UI。
- **Transport = 系统 `ssh` 经本地 PTY**，重连 = 重新执行 `tmux new -As <session>`；keepalive `ServerAliveInterval=15` / `ServerAliveCountMax=3`。
- **非目标别加**：输出解析、agent 状态感知、多机看板、Mosh/ET、服务端自研组件。

## 开发环境（当前为 headless Linux aarch64 服务器）

- Rust 工具链经 rustup 安装，非交互 shell 需要 `export PATH="$HOME/.cargo/bin:$PATH"`。
- **没有 webkit2gtk，本机不能构建/运行 Tauri GUI**——GUI 构建只在 GitHub Actions（windows-latest）。不要尝试在本地装 GUI 依赖。
- Node 24 + npm，前端依赖已装（`node_modules` 在）。

## 常用命令

```bash
npm ci                # 安装前端依赖
cargo test            # core + src-tauri 全部单测/集成测试（提交前必跑）
npm test              # vitest（前端纯函数单测，提交前必跑）
npm run build         # tsc 严格模式 + vite build（提交前必跑）
npx tauri dev         # 开发模式运行 GUI（仅 Windows/有 webkit2gtk 的机器）
npx tauri build       # 打包发布版（仅 Windows/有 webkit2gtk 的机器）
```

`src-tauri` 的真实编译由 CI 首次保证；本地只保证 `cargo test`（不含 GUI 链接）。

## 前人踩过的坑（SDD ledger 沉淀，勿重蹈）

1. `portable_pty` 对**被信号杀死的子进程报告 exit code 1**，信号本身拿不到——`classify_ssh(1, "")` 不等于"干净退出"。
2. `PtySession` 没有 `Drop`——每条路径（含错误路径）都必须显式 `kill()` + `wait()`，否则孤儿 ssh 进程。
3. `wait()` 无限阻塞——先 kill 再 wait。
4. ssh 失败分类的可靠依据是 `probe_session` 的 **stderr**，不是 PTY 流。
5. 静默断线（无 stderr、exit 0/1）用 `classify_unexpected_exit(exit_code, connected_for)` 的 60 秒存活启发式判定。
6. 重连退避延迟由 `backoff::decide()` 返回，runner 不得自行再翻倍（防止双重推进）。

## 约定

- Conventional Commits（`feat:` / `fix:` / `chore:` / `docs:` / `ci:`），英文小写。
- TDD：先写会失败的测试，再写最小实现；提交粒度一个逻辑变更。
- 最小 diff：不加投机抽象、不加没被要求的配置项。

## 当前状态（2026-09-12）

- 版本 `0.1.3`；计划的 10 个任务全部完成并合入 `master`；工作分支 `feat/core`。
- 2026-09-12 新增：应用内配置编辑（`save_config` 命令 + 设置面板）、agent 品牌图标、UI 精修（`5836d6c`），待用户在 Windows 上目视确认。
- CI：push 到 `master` 或手动 dispatch 触发 Windows 构建，产物发 GitHub Releases。
- `examples/config.example.json` 里 `REPLACE_WITH_*` 占位符由用户填真实值（host、agent 启动命令）。
- 遗留：无已知未完成计划任务；新需求按"设计确认 → 实现"流程走，勿直接动代码。
- 跨机器协作进展见 `WORKLOG.md`（每次会话收工时更新）。

## 收工规矩

每次会话结束前，无论任务是否完成，必须：

1. 更新 WORKLOG.md（倒序排列，最新在顶部）：写日期、本次做了什么、当前状态、下一步计划、踩过的坑
2. git commit（可用 docs(worklog): 前缀），有远端则 push

当用户只说"收工"两个字时，立即执行以上流程，除此之外不做任何其他事。
