# Agent 会话持久化桌面客户端 — 设计文档

- 日期：2026-09-11
- 状态：设计已与用户逐节确认，待用户审阅本文档
- 路径：`docs/superpowers/specs/2026-09-11-agent-session-client-design.md`
- 流程：superpowers `brainstorming`（architectural path）

---

## 1. 背景与问题

用户在 Windows 上通过网络（cmd / SSH）连到远程 Linux 服务器，在服务器上以**交互式 CLI** 方式运行 agent（Hermes Agent、DeepSeek Harness）。

问题：Windows 端网络随时可能中断——既有短暂抖动/换 Wi‑Fi/合盖休眠，也有长时间彻底离线。网络一断，SSH 连接死掉，终端界面消失，用户重开后无法回到原来的工作现场。

用户希望：**断网后重开客户端，界面回到断线前的会话，并能看到断线前的输出。**

## 2. 目标与非目标

### 目标（MVP）

1. 在客户端里新建 / 接入远程 agent 会话（Hermes Agent、DeepSeek Harness）。
2. 网络中断后，服务端 agent 进程不中断。
3. 客户端重连后，重新附身到同一会话，并能看到断线前的终端输出。
4. 客户端界面为远端终端的**原样镜像**（不解析 agent 输出）。

### 非目标（明确砍掉，YAGNI）

- 不做输出解析、不做聊天式界面。
- 不做 agent 状态感知（working/blocked/done）。
- 不做多机 / 多 agent 看板与调度。
- 不做本地布局持久化（窗口位置、拖拽面板、输入草稿）。
- 不做 Mosh / Eternal Terminal 传输层。
- 不做遥测、自动更新、崩溃上报。
- 服务端不部署任何自研组件（只依赖系统 `tmux`）。

## 3. 使用场景

- **S1 新建会话**：用户选一台 host + 一个 agent 预设，客户端开一个终端，远端在该会话中启动 agent。
- **S2 短抖动**：网络闪断/漫游，客户端自动重连，恢复同一会话与画面。
- **S3 长离线**：用户关机或离线数小时；重新打开客户端时，服务端 agent 仍在运行，客户端重连即恢复。
- **S4 主动结束**：用户显式结束会话，远端会话被销毁。

## 4. 约束与前提

- 客户端系统：Windows（自用）。
- 服务端：Linux，需预装 `tmux`。
- agent 形态：交互式 CLI（本次两者均为 CLI）。
- 传输：系统 OpenSSH（复用用户已有的 `~/.ssh/config`、密钥、ssh-agent）。
- 身份与自用定位：不涉及商用许可红线；若日后引入第三方二进制（如 rmux/herdr），需重新评估许可。
- 已知环境缺口：当前开发/构建环境（`/home/ubuntu`）**没有 Rust/cargo**；Tauri 需要 Rust + 对应平台构建依赖（Windows 侧需 Rust + MSVC Build Tools）。

## 5. 架构总览

```
┌──────────────── Windows 客户端 (Tauri 2) ────────────────┐
│  React + xterm.js  ←── 字节流 ──►  Rust 后端              │
│                                     ├─ SessionManager    │
│                                     ├─ Transport (PTY)   │
│                                     └─ Reconnect         │
└─────────────────────────┬────────────────────────────────┘
              ssh -t  "tmux new -As <name> '<agentCmd>'"
┌─────────────────────────┴──────── Linux 服务器 ───────────┐
│  tmux server ── session: hermes / harness / <project>     │
│      └─ 交互式 CLI（hermes chat / deepseek-harness …）     │
└───────────────────────────────────────────────────────────┘
```

**核心原则：客户端不缓存 agent 输出，服务端 `tmux` 是唯一事实源。** 客户端只负责渲染字节流、维持连接、在断开后重连。

## 6. 组件职责

### 6.1 UI 层（React + xterm.js）

- 渲染终端字节流；显示会话列表与连接状态。
- **不解析** agent 输出。
- 使用 `xterm-addon-fit` 自适应尺寸；尺寸变化时通知 Rust 层。
- 顶层状态：`connecting | connected | retrying | exited | closed`。

### 6.2 SessionManager（Rust）

- 读取配置（hosts + agent 预设），维护会话元数据：`host`、`sessionName`、`agentCmd`、`state`。
- `sessionName` 必须**持久化**到本地元数据，保证重连时目标名不变。
- 多个会话 = 多个独立实例，互不干扰。

### 6.3 Transport（Rust）

- 用本地 PTY 启动系统 `ssh`，双向转发字节流。
- 强制注入保活参数：`-o ServerAliveInterval=15 -o ServerAliveCountMax=3`，使 ssh 在网络黑洞下约 45 秒内自行退出。
- 转发终端 resize 到 PTY（`ssh -t` 会将其传到远端）。
- Windows 下使用 ConPTY；统一 UTF‑8。

### 6.4 Reconnect（Rust）

- 监听 ssh 进程退出。
- 按退避序列重试：1s → 2s → 4s → 8s → … 封顶 30s。
- 重连即重新执行同一条 `tmux new -As <sessionName>`。

### 6.5 配置

- 单个 JSON/TOML 文件，内容：
  - `hosts[]`：`{ name, host, [user], [extraSshArgs] }`
  - `agents[]`：`{ id, label, cmd }`（如 `hermes`、`harness`）
  - 可选的默认 `sessionName` 规则。
- 启动时按 schema 校验，字段错误需报出具体字段名。

## 7. 会话生命周期与数据流

状态机：**新建 → 已连接 → 断开 → 重连 → 结束**

1. **新建 / 接入**：选 host + agent 预设，拼出
   `ssh -t <host> "tmux new -As <sessionName> '<agentCmd>'"`
   并在本地 PTY 中 spawn。`new -As` 语义：会话不存在则创建并执行 agent，存在则直接附身——**同一条命令同时覆盖"新建"与"重连"**。
2. **正常使用**：PTY ↔ xterm.js 双向字节流转发。
3. **断网**：ssh 退出 → Reconnect 捕获 → UI 显示"已断开，重连中"。服务端 tmux 与 agent 持续运行。
4. **重连**：退避后重新 spawn 同一命令；成功后先让 xterm.js 清屏，再由 tmux 重绘当前屏。历史输出通过 tmux 自身 scrollback（copy-mode 上翻）获取。
5. **结束**：关闭标签页 = 仅杀本地 ssh，远端会话仍在；只有显式"结束会话"才执行 `tmux kill-session -t <sessionName>`。

### 关键细节

- **agent 退出后的会话语义**：若 agent CLI 退出导致 pane 关闭、会话随之销毁，则 `new -As` 会新建一个空会话。为避免"看起来恢复了其实是新会话"，设计采用 `tmux set-option remain-on-exit on`，使 pane 死亡后会话保留，客户端据此把状态标为 `exited` 并保留最后画面。
- **命令拼装与转义**：`agentCmd` 需经 shell 引用后嵌入远端命令串。这是最易出 bug 的点，必须有单元测试覆盖（含引号、空格、`$` 等）。

## 8. 错误处理矩阵

| 故障 | 判定依据 | 处理 |
|---|---|---|
| 网络不可达 / 黑洞 | ssh 因保活超时退出 | 退避重连，UI 显示重试次数 |
| 认证失败（key/密码） | ssh stderr + 退出码 | **不重试**，UI 显示 ssh 报错原因 |
| 服务端缺 `tmux` | stderr 含 `command not found` | 提示缺 tmux + 安装指引，不重试 |
| 原会话已不存在（服务器重启） | 附身时未命中已有会话 | 显式提示"原会话已不在，已新建" |
| agent CLI 启动失败 / 退出 | pane 死亡（`remain-on-exit`） | 状态置 `exited`，保留最后画面 |
| 配置字段错误 | 启动时 schema 校验 | 报具体字段，拒绝启动该会话 |
| 终端尺寸变化 | UI resize 事件 | 转发到 PTY，避免 tmux 显示错位 |

## 9. 测试策略

原则：**测试中不真连网络**。

- **Rust 单元测试**：命令拼装与转义；退避序列；ssh 退出码/stderr 分类（网络 vs 认证 vs 缺 tmux）。
- **Rust 集成测试**：用 fake `ssh` shim 脚本（可控输出、可控退出/挂起）验证 spawn / resize / 断线 / 重连路径。
- **UI 测试**：连接状态机（`connected | disconnected | retrying | exited`）；xterm.js 挂载与清屏时序。
- **端到端手工冒烟**：真机 `tmux` 内跑 `while true; do date; sleep 1; done` → 断 Wi‑Fi → 重连验证续上；再分别对 Hermes Agent、DeepSeek Harness 各跑一次。
- **TDD**：SessionManager / Reconnect / 命令拼装等逻辑层先写测试再实现（遵循 superpowers `test-driven-development`）。

## 10. 技术选型与理由

| 选择 | 理由 |
|---|---|
| Tauri 2 + React + xterm.js | 用户选定形态；体积小、原生窗口、xterm.js 是终端渲染成熟方案 |
| 系统 `ssh`（而非内嵌 Rust SSH 库） | 代码最少，直接复用 `~/.ssh/config`、密钥、ssh-agent；用户已确认 |
| 服务端 `tmux` | 满足"进程与终端解绑"的最成熟轮子，几乎零部署成本 |

### 被否决的方案

- **方案 B（rmux 自包含守护）**：许可干净、SDK 好用、恢复更确定，但对"仅需会话+输出"的 MVP 属于过度设计。
- **方案 C（Eternal Terminal）**：能回放完整 scrollback，但需服务端守护进程 + 额外端口，运维成本最高。

> 若日后实测发现 tmux 的 scrollback 恢复不够用，可再评估 B/C。

## 11. 前置条件

- **服务端**：`tmux`（`sudo dnf install tmux` / `sudo apt install tmux`，视发行版）。
- **客户端构建**：Rust 工具链 + 平台构建依赖（Windows：Rust + MSVC Build Tools + WebView2）。
- **开发环境现状**：`/home/ubuntu` 无 Rust/cargo，需先安装。

## 12. 里程碑（粗粒度，细节留待 writing-plans）

1. M1：Rust 逻辑层（命令拼装、SessionManager、Reconnect）+ 单元测试。
2. M2：Transport（PTY + 系统 ssh）+ 集成测试（fake ssh shim）。
3. M3：Tauri 壳 + xterm.js UI + 状态机。
4. M4：配置加载与校验。
5. M5：端到端冒烟（假 agent → 真 Hermes / DeepSeek Harness）。

## 13. 验收标准

1. 能在客户端新建会话并在远端启动指定 agent。
2. 断开网络后，远端 agent 进程存活、不中断。
3. 恢复网络后客户端自动重连到**同一** `sessionName`，并显示断线前的屏幕内容；历史可通过 scrollback 上翻。
4. 客户端关闭后重开，能重新接入仍在运行的会话。
5. 认证失败不会被无脑重试；缺 tmux 有明确提示。
6. 上述逻辑有单元/集成测试覆盖，且测试不依赖真实网络。

## 14. 风险与未决问题

- **R1**：`tmux` 的 scrollback 只保留有限行数，且重连时只重绘当前屏；若用户期望"无限历史回放"，需引入 ET 或本地缓存（当前判定为非目标）。
- **R2**：Windows 上系统 `ssh` 的路径/版本差异（Win10+ 自带 OpenSSH），需在实现阶段验证。
- **R3**：`remain-on-exit` 改变 tmux 默认语义，需确认不会干扰 agent 正常交互。
- **Q1**：会话命名规则尚未定。**建议默认** `<agent>-<project>`（`project` 由用户新建会话时填写，缺省回退为 host 名），并允许手动覆盖——待确认。
- **Q2**：目标 host 清单，以及 Hermes / DeepSeek Harness 的确切启动命令，需用户提供后才能落到配置里。

## 15. 变更记录

- 2026-09-11：初版，逐节与用户确认（架构与组件 → 会话生命周期与数据流 → 错误处理与测试策略）。
