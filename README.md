# Agent Sessions Client

![build](https://github.com/owlshift/agent-session-client/actions/workflows/build.yml/badge.svg)

一个 Windows 桌面客户端，用来**在断网/关机后重新接回远端仍在运行的交互式 CLI agent**，并保留断线前的终端画面。

在服务器上用 `tmux` 跑 agent（Hermes Agent、DeepSeek Harness 等），客户端只负责把这台机器的终端**原样镜像**到本地窗口。网络断了、笔记本合盖了、Wi-Fi 换了——服务器上的 agent 不受影响；重新打开客户端即回到同一会话。

> 设计原则：**客户端不缓存任何 agent 输出，服务端 `tmux` 是唯一事实源。** 客户端只做渲染、维持连接、断开后重连。

---

## 它解决什么问题

Windows 端网络随时可能中断——短暂抖动、换 Wi-Fi、休眠，或长时间彻底离线。网络一断，SSH 连接死掉，终端界面消失，重开后无法回到原来的工作现场。

Agent Sessions 把"会话"从易碎的 SSH 连接里解耦出来：

| 场景 | 连接断开后 | 重连后 |
|---|---|---|
| 短抖动 | 状态 `retrying`，保留最后画面 | 回到 `connected`，tmux 重绘同一会话 |
| 长离线（关机数小时） | agent 在服务端继续跑 | 接回同一会话，历史仍在 |
| agent 自己退出 | 状态 `exited` | 不自动重启，由用户决定 |

## 非目标（明确不做）

- 不解析 agent 输出、不做聊天式界面、不做状态感知（working/blocked/done）
- 不做多机/多 agent 看板与调度
- 不做窗口位置等本地布局持久化
- 不引入 Mosh / Eternal Terminal 之类的传输层
- **服务端不部署任何自研组件**——只依赖系统 `tmux`

---

## 架构

```
┌──────────────── Windows 客户端 (Tauri 2) ────────────────┐
│  React + xterm.js  ←── 字节流 ──►  Rust 后端              │
│                                     ├─ SessionManager    │
│                                     ├─ Transport (PTY)   │
│                                     └─ Reconnect         │
└─────────────────────────┬────────────────────────────────┘
              ssh -t "tmux new -As <session> '<agentCmd>'"
┌─────────────────────────┴──────── Linux 服务器 ───────────┐
│  tmux server ── session: <session>                        │
│      └─ 交互式 CLI（hermes chat / deepseek-harness …）     │
└───────────────────────────────────────────────────────────┘
```

- **Rust 后端**用本地 PTY 启动**系统 `ssh`**，把远端 `tmux new -As <session> '<agentCmd>'` 的字节流双向转发给前端。`-A` 保证"存在即附身、不存在即新建"。
- **xterm.js 只渲染字节流**，不解析内容。
- **`core` crate 不依赖 GUI/Tauri**，纯逻辑 + PTY，可在 Linux 上单测；`src-tauri` 只做接线与事件转发。

### 重连逻辑（`core`）

| 环节 | 行为 |
|---|---|
| 保活 | SSH 固定注入 `-o ServerAliveInterval=15 -o ServerAliveCountMax=3` |
| 退避 | 起始 1000ms，每次 ×2，上限 30000ms；连接成功后复位 |
| 可重试判定 | 仅"网络类"失败重试；认证失败 / 缺 tmux / 未知错误直接放弃 |
| 意外退出 | stderr 已并入 PTY 流，用「退出码 255 + 在线时长 ≥ 60s」启发式判定为网络掉线 |

### 会话状态

`connecting` → `connected` → （断线）`retrying` → `connected` … → `closed`

- `exited` 是终态：agent 退出或不可重试的失败。会话行上的 **✕** 会关掉它（进入 `closed`）：
  只断开本地 ssh，**远端 tmux 会话保留**，之后点同一个 agent 即可接回。

---

## 快速开始

### 1. 服务端（Linux）

只需要 `tmux`：

```bash
sudo apt install tmux     # Debian/Ubuntu
sudo dnf install tmux     # Fedora/RHEL
```

客户端复用你现有的 SSH 配置：`~/.ssh/config`、密钥、ssh-agent 均照常生效。

### 2. 客户端（Windows）

从 [Releases](../../releases) 下载最新的安装包安装即可。首次打开若没有配置，界面会提示「未找到配置文件」并引导你生成。

### 3. 从源码构建

前置：Node 20+、Rust stable，以及对应平台的 Tauri 依赖（Windows 需 WebView2）。

```bash
npm ci                 # 安装前端依赖
npx tauri dev          # 开发模式（热重载）
npx tauri build        # 打包发布版
```

> Linux 上构建还需 `webkit2gtk` 等系统库（`pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev`）。

---

## 配置

配置文件是一个 JSON，包含 **host 清单**和 **agent 模板**：

```json
{
  "hosts": [
    { "name": "main", "host": "10.0.0.1", "user": "ubuntu" }
  ],
  "agents": [
    { "id": "hermes", "label": "Hermes Agent", "cmd": "hermes chat" },
    { "id": "dsh", "label": "DeepSeek Harness", "cmd": "dsh" },
  ]
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `hosts[].name` | ✅ | 界面里显示的 host 名 |
| `hosts[].host` | ✅ | 主机名或 IP |
| `hosts[].user` | | SSH 用户名；省略则用 `~/.ssh/config` 里的默认值 |
| `hosts[].extra_ssh_args` | | 追加的 ssh 参数，如 `["-p", "2222"]` |
| `agents[].id` / `label` | ✅ | agent 标识与显示名 |
| `agents[].cmd` | ✅ | 远端启动命令，例如 `hermes chat` |

远端 tmux 会话名按 **agent** 生成（`<agent>`；指定 `project` 时为 `<agent>-<project>`），
因此同一 host 上的不同 agent 各自使用独立会话，互不干扰。界面里每个 host×agent 组合
都会渲染成一个按钮，点哪个就用哪个。

参考模板见 [`examples/config.example.json`](examples/config.example.json)。

> 也可以在应用内点侧栏底部的「**⚙ 编辑配置**」直接增删改 host / agent，保存时会校验并写入用户配置目录，无需手动编辑 JSON。
>
> 设置面板里还能**探测服务器上装了哪些 agent**：选一台已保存的 host，点「🔍 探测已装 agent」，
> 命中的会列出来；点「加入 / 全部加入」写进 Agent 列表，再点保存才生效（不会自动改配置）。
> 探测只做一次 `command -v`，用的就是会话启动时那个非登录 shell 环境 —— 能探到 = 真能启动。

> ⚠️ **真实配置值（服务器地址、用户名、agent 命令）永远不要提交到仓库。** 仓库中的示例始终保持 `REPLACE_WITH_*` 占位符。

### 配置从哪来

启动时按**优先级顺序**查找，取第一个存在的文件：

1. 环境变量 `VITE_AGENT_SESSION_CONFIG` 指定的路径（可选覆盖）
2. `<当前工作目录>/config.json`
3. `<exe 所在目录>/config.json`
4. **用户配置目录** `<app_config_dir>/config.json`（Windows 通常为 `%APPDATA%\dev.local.agent-session-client\config.json`）
5. `<exe 所在目录>/examples/config.example.json`
6. `<当前工作目录>/examples/config.example.json`

若都找不到，界面会显示**已查找的全部路径**并提供「**生成示例配置**」（写入第 4 项）与「重新加载」；若文件存在但字段非法，会显示**文件路径与具体错误**（如 `hosts[0].name: must not be empty`）。

开发模式下直接放一个 `config.json` 在项目根目录即可，或用环境变量显式指定（Vite 构建期变量）：

```powershell
$env:VITE_AGENT_SESSION_CONFIG="config.json"
npx tauri dev
```

---

## 开发

```bash
npm test                      # 前端单测（vitest）
npm run build                 # tsc + vite 构建
cargo test -p session_core    # 核心逻辑单测（纯 Linux 可跑）
cargo check -p agent-session-client
```

`core` 承担全部可测逻辑：SSH/tmux 命令拼装、退出分类、退避重连、状态机、PTY 传输、配置解析与发现。新增逻辑请优先放进 `core` 并补单测，`src-tauri` 保持"只接线"。

### 目录结构

```
core/                 # 纯 Rust：无 GUI/Tauri 依赖，Linux 可单测
  src/command.rs      #   ssh/tmux 命令拼装 + shell 引用
  src/exit.rs         #   ssh 结果分类（网络/认证/缺 tmux）
  src/backoff.rs      #   退避序列 1s→30s
  src/reconnect.rs    #   是否重试的决策 + 意外退出启发式
  src/session.rs      #   连接状态机
  src/transport.rs    #   PTY 启动/读写/resize/kill，UTF-8 分块安全
  src/config.rs       #   配置解析与校验
  src/discovery.rs    #   多位置配置发现 + 首启模板
src-tauri/            # Tauri 2 应用：IPC 命令 + 会话运行器
src/                  # React + TypeScript + xterm.js 前端
docs/                 # 设计文档与手工冒烟清单
```

---

## 文档

- [`docs/smoke-test.md`](docs/smoke-test.md) — 手工冒烟清单（连通、断线恢复、失败分支）
- [`docs/superpowers/specs/`](docs/superpowers/specs/) — 设计文档
- [`docs/superpowers/plans/`](docs/superpowers/plans/) — 实现计划

## 构建与发布

CI 见 [`.github/workflows/build.yml`](.github/workflows/build.yml)：推送到 `master`（或手动 dispatch）会在 `windows-latest` 上构建，并用 `tauri-action` 发布 Release。版本号取自 `src-tauri/tauri.conf.json`，tag 为 `v<version>`；**发版前记得提升版本号**，否则会与已存在的 tag 冲突。

## 许可

私有项目，未声明开源许可。
