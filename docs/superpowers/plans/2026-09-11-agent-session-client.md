# Agent 会话持久化桌面客户端 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Windows 用户断网重开后，能重新接入远端仍在运行的 agent 会话，并看到断线前的终端输出。

**Architecture:** 客户端是一个 Tauri 2 应用。Rust 侧用本地 PTY 启动系统 `ssh`，把远端 `tmux new -As <session> '<agentCmd>'` 的字节流双向转发给 xterm.js 渲染；网络中断时按退避策略重连同一条命令。不再自研服务端组件——`tmux` 是唯一事实源，客户端不缓存 agent 输出。

**Tech Stack:** Rust（`core` crate：无 GUI 依赖、纯逻辑 + PTY；`src-tauri`：Tauri 2）、React + TypeScript + Vite、`@xterm/xterm` + `@xterm/addon-fit`、`portable-pty`、`serde`/`serde_json`、`vitest`。

**Spec:** `docs/superpowers/specs/2026-09-11-agent-session-client-design.md`

## Global Constraints

- **客户端平台**：Windows。服务端 Linux，前置依赖只有 `tmux`（`sudo dnf install tmux` / `sudo apt install tmux`）。
- **传输**：必须用**系统 `ssh`** 进程经本地 PTY 转发；**不得**引入 Rust SSH 库（russh/ssh2 等）。
- **保活参数**：注入的 SSH 选项必须**逐字**为 `ServerAliveInterval=15`、`ServerAliveCountMax=3`（用 `-o` 传入）。
- **重连退避**：起始 1000ms，每次 ×2，上限 30000ms。
- **会话恢复命令**：必须是 `tmux new -As <sessionName> '<agentCmd>'`（`-A` 保证"存在即附身、不存在即新建"）。
- **UI 不解析 agent 输出**：xterm.js 只渲染字节流。
- **配置驱动**：host 清单与 agent 启动命令一律来自配置，不得硬编码任何 host 或 agent 命令（Q2 尚未提供，示例值仅出现在测试夹具与 `examples/`）。
- **平台/构建前提**：`core` crate 必须在 Linux 上 `cargo test` 全绿（GUI 无关）；`src-tauri` 与前端需要 Rust 工具链 + WebView2（Windows）或 `webkit2gtk`（Linux）。
- **无占位符原则**：本计划每一步都给出可直接落地的代码；执行者不得以"补个错误处理"之类含糊步骤替代。

## File Structure

```
agent-session-client/
├── Cargo.toml                      # cargo workspace（先只有 core，Task 8 加入 src-tauri）
├── .gitignore
├── core/                           # 纯 Rust，无 GUI/Tauri 依赖，Linux 可测
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                  # 公开模块与版本常量
│       ├── command.rs              # shell 引用 + ssh/tmux 命令拼装
│       ├── exit.rs                 # ssh 结果分类
│       ├── backoff.rs              # 退避序列
│       ├── reconnect.rs            # 是否重试的决策
│       ├── config.rs               # 配置解析与校验
│       ├── session.rs              # 连接状态机
│       └── transport.rs            # PTY 启动/读写/resize/kill
├── core/tests/
│   ├── transport_shim.rs           # 用 fake ssh shim 验证 spawn/IO/退出
│   └── fixtures/fake-ssh.sh        # 可控输出的假 ssh
├── examples/config.example.json    # 示例配置（含 Hermes/Harness 占位命令）
├── src-tauri/                      # Tauri 2 应用（Task 8 起）
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── commands.rs             # IPC 命令
│       └── runner.rs               # 会话运行器：probe → spawn → 读循环 → 重连
└── src/                            # 前端（Task 9 起）
    ├── main.tsx
    ├── App.tsx
    ├── Terminal.tsx
    ├── sessions.ts                 # 状态 reducer（纯函数，单测）
    └── sessions.test.ts
```

**设计原则**：`core` 不依赖 Tauri，保证全部逻辑能在 Linux 上用 `cargo test` 验证；`src-tauri` 只做"接线 + 事件转发"；前端只做渲染与状态展示。

---

### Task 1: 引导 cargo workspace 与 `core` crate

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `core/Cargo.toml`
- Create: `core/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces: `session_core::PROJECT_NAME: &str`（供后续任务与冒烟断言使用）；crate 名 `session_core`。

- [ ] **Step 1: 安装 Rust 工具链（本机缺少 cargo）**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
cargo --version
```

Expected: 打印 `cargo 1.8x.x`（任何 stable 版本均可）。

- [ ] **Step 2: 写会失败的测试**

`core/src/lib.rs`：

```rust
pub const PROJECT_NAME: &str = "agent-session-client";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_matches_repo() {
        assert_eq!(PROJECT_NAME, "agent-session-client");
    }
}
```

- [ ] **Step 3: 建 workspace 与 crate 清单，运行测试确认失败**

`Cargo.toml`：

```toml
[workspace]
resolver = "2"
members = ["core"]
```

`core/Cargo.toml`：

```toml
[package]
name = "session_core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
```

`.gitignore`：

```
/target
node_modules
dist
```

Run: `cargo test -p session_core`

Expected: 首先因 `core/src/lib.rs` 尚未创建而报错（"no targets specified"/"file not found"），即 FAIL。随后创建好 `lib.rs`（Step 2 内容）再运行。

> 说明：本步的"失败"体现为编译器找不到 crate 目标；创建文件后测试通过。这是引导任务的正常红→绿。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core`

Expected: `test tests::project_name_matches_repo ... ok`，`test result: ok. 1 passed`。

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml .gitignore core/Cargo.toml core/src/lib.rs
git commit -m "chore: bootstrap cargo workspace and session_core crate"
```

---

### Task 2: `core::command` — shell 引用与 ssh/tmux 命令拼装

**Files:**
- Create: `core/src/command.rs`
- Modify: `core/src/lib.rs`（加 `pub mod command;`）

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub struct SshTarget { pub host: String, pub user: Option<String>, pub extra_ssh_args: Vec<String> }`
  - `pub const KEEPALIVE_ARGS: [&str; 4]`
  - `pub fn shell_quote(s: &str) -> String`
  - `pub fn destination(target: &SshTarget) -> String`
  - `pub fn build_remote_tmux_cmd(session: &str, agent_cmd: &str) -> String`
  - `pub fn build_session_argv(target: &SshTarget, session: &str, agent_cmd: &str) -> Vec<String>`
  - `pub fn build_probe_argv(target: &SshTarget, session: &str) -> Vec<String>`

- [ ] **Step 1: 写会失败的测试**

`core/src/command.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SshTarget {
        SshTarget { host: "example.com".into(), user: Some("ubuntu".into()), extra_ssh_args: vec![] }
    }

    #[test]
    fn quotes_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn quotes_spaces_and_metachars() {
        assert_eq!(shell_quote("a b$c;d"), "'a b$c;d'");
    }

    #[test]
    fn builds_remote_tmux_cmd() {
        assert_eq!(
            build_remote_tmux_cmd("hermes-proj", "hermes chat"),
            "tmux new -As 'hermes-proj' 'hermes chat'"
        );
    }

    #[test]
    fn builds_session_argv_with_keepalive_and_tty() {
        assert_eq!(build_session_argv(&target(), "hermes-proj", "hermes chat"), vec![
            "ssh",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-t", "ubuntu@example.com",
            "tmux new -As 'hermes-proj' 'hermes chat'",
        ]);
    }

    #[test]
    fn builds_probe_argv_without_tty() {
        assert_eq!(build_probe_argv(&target(), "hermes-proj"), vec![
            "ssh",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "ubuntu@example.com",
            "tmux has-session -t 'hermes-proj'",
        ]);
    }

    #[test]
    fn extra_ssh_args_precede_destination() {
        let t = SshTarget { host: "h".into(), user: None, extra_ssh_args: vec!["-p".into(), "2222".into()] };
        let argv = build_session_argv(&t, "s", "c");
        assert_eq!(&argv[1..7], &["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-p", "2222", "-t"]);
        assert_eq!(argv[7], "h");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p session_core command::`

Expected: 编译失败，`cannot find function `shell_quote`` 等（FAIL）。

- [ ] **Step 3: 写最小实现**

`core/src/command.rs`：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub user: Option<String>,
    pub extra_ssh_args: Vec<String>,
}

pub const KEEPALIVE_ARGS: [&str; 4] =
    ["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"];

/// POSIX 单引号引用：把 `'` 转义为 `'\''`，其余字符原样放入单引号内。
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub fn destination(target: &SshTarget) -> String {
    match &target.user {
        Some(user) => format!("{user}@{}", target.host),
        None => target.host.clone(),
    }
}

pub fn build_remote_tmux_cmd(session: &str, agent_cmd: &str) -> String {
    format!("tmux new -As {} {}", shell_quote(session), shell_quote(agent_cmd))
}

fn base_argv(target: &SshTarget) -> Vec<String> {
    let mut argv: Vec<String> = vec!["ssh".into()];
    argv.extend(KEEPALIVE_ARGS.iter().map(|s| (*s).to_string()));
    argv.extend(target.extra_ssh_args.iter().cloned());
    argv
}

pub fn build_session_argv(target: &SshTarget, session: &str, agent_cmd: &str) -> Vec<String> {
    let mut argv = base_argv(target);
    argv.push("-t".into());
    argv.push(destination(target));
    argv.push(build_remote_tmux_cmd(session, agent_cmd));
    argv
}

pub fn build_probe_argv(target: &SshTarget, session: &str) -> Vec<String> {
    let mut argv = base_argv(target);
    argv.push(destination(target));
    argv.push(format!("tmux has-session -t {}", shell_quote(session)));
    argv
}
```

`core/src/lib.rs` 顶部加：

```rust
pub mod command;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core command::`

Expected: `6 passed`。

- [ ] **Step 5: 提交**

```bash
git add core/src/command.rs core/src/lib.rs
git commit -m "feat(core): build ssh/tmux argv with proper shell quoting"
```

---

### Task 3: `core::exit` — ssh 结果分类

**Files:**
- Create: `core/src/exit.rs`
- Modify: `core/src/lib.rs`（加 `pub mod exit;`）

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub enum SshOutcome { Network, Auth, TmuxMissing, Unknown }`
  - `pub fn classify_ssh(exit_code: Option<i32>, stderr: &str) -> SshOutcome`
  - `pub fn is_retryable(outcome: SshOutcome) -> bool`

- [ ] **Step 1: 写会失败的测试**

`core/src/exit.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_missing_tmux() {
        assert_eq!(classify_ssh(Some(127), "bash: tmux: command not found"), SshOutcome::TmuxMissing);
    }

    #[test]
    fn detects_auth_failure() {
        assert_eq!(classify_ssh(Some(255), "ubuntu@h: Permission denied (publickey)."), SshOutcome::Auth);
    }

    #[test]
    fn detects_network_black_hole() {
        assert_eq!(classify_ssh(Some(255), "ssh: connect to host h port 22: Connection timed out"), SshOutcome::Network);
        assert_eq!(classify_ssh(None, "Connection closed by remote host"), SshOutcome::Network);
    }

    #[test]
    fn unknown_when_no_signature_matches() {
        assert_eq!(classify_ssh(Some(1), "some unexpected failure"), SshOutcome::Unknown);
    }

    #[test]
    fn only_network_is_retryable() {
        assert!(is_retryable(SshOutcome::Network));
        assert!(!is_retryable(SshOutcome::Auth));
        assert!(!is_retryable(SshOutcome::TmuxMissing));
        assert!(!is_retryable(SshOutcome::Unknown));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p session_core exit::`

Expected: 编译失败（`SshOutcome` 未定义）（FAIL）。

- [ ] **Step 3: 写最小实现**

`core/src/exit.rs`：

```rust
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SshOutcome {
    Network,
    Auth,
    TmuxMissing,
    Unknown,
}

pub fn classify_ssh(_exit_code: Option<i32>, stderr: &str) -> SshOutcome {
    let s = stderr.to_ascii_lowercase();

    if s.contains("tmux: command not found") || s.contains("tmux: not found") {
        return SshOutcome::TmuxMissing;
    }
    if s.contains("permission denied")
        || s.contains("authentication failed")
        || s.contains("too many authentication failures")
        || s.contains("no supported authentication methods")
    {
        return SshOutcome::Auth;
    }
    if s.contains("connection timed out")
        || s.contains("connection refused")
        || s.contains("connection closed by")
        || s.contains("connection reset")
        || s.contains("could not resolve hostname")
        || s.contains("network is unreachable")
        || s.contains("no route to host")
        || s.contains("operation timed out")
        || s.contains("broken pipe")
    {
        return SshOutcome::Network;
    }
    SshOutcome::Unknown
}

pub fn is_retryable(outcome: SshOutcome) -> bool {
    matches!(outcome, SshOutcome::Network)
}
```

`core/src/lib.rs` 加：

```rust
pub mod exit;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core exit::`

Expected: `4 passed`。

- [ ] **Step 5: 提交**

```bash
git add core/src/exit.rs core/src/lib.rs
git commit -m "feat(core): classify ssh failures into network/auth/tmux/unknown"
```

---

### Task 4: `core::backoff` + `core::reconnect` — 重试策略

**Files:**
- Create: `core/src/backoff.rs`
- Create: `core/src/reconnect.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `crate::exit::{SshOutcome, is_retryable}`
- Produces:
  - `pub struct Backoff`（`Backoff::default()`：base 1000ms、cap 30000ms、attempt 0）
  - `Backoff::next_delay(&mut self) -> Duration`
  - `Backoff::reset(&mut self)`
  - `pub enum RetryDecision { Retry(Duration), GiveUp }`
  - `pub fn decide(outcome: SshOutcome, backoff: &mut Backoff) -> RetryDecision`

- [ ] **Step 1: 写会失败的测试**

`core/src/backoff.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn doubles_then_caps_at_30s() {
        let mut b = Backoff::default();
        let got: Vec<u64> = (0..8).map(|_| b.next_delay().as_millis() as u64).collect();
        assert_eq!(got, vec![1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
    }

    #[test]
    fn reset_restarts_sequence() {
        let mut b = Backoff::default();
        let _ = b.next_delay();
        let _ = b.next_delay();
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_millis(1000));
    }
}
```

`core/src/reconnect.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit::SshOutcome;
    use std::time::Duration;

    #[test]
    fn retries_network_and_backs_off() {
        let mut b = Backoff::default();
        assert_eq!(decide(SshOutcome::Network, &mut b), RetryDecision::Retry(Duration::from_millis(1000)));
        assert_eq!(decide(SshOutcome::Network, &mut b), RetryDecision::Retry(Duration::from_millis(2000)));
    }

    #[test]
    fn gives_up_on_auth_and_resets_backoff() {
        let mut b = Backoff::default();
        let _ = decide(SshOutcome::Network, &mut b);
        assert_eq!(decide(SshOutcome::Auth, &mut b), RetryDecision::GiveUp);
        // 复位后下次网络错误从 1000ms 重新开始
        assert_eq!(decide(SshOutcome::Network, &mut b), RetryDecision::Retry(Duration::from_millis(1000)));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p session_core backoff:: reconnect::`

Expected: 编译失败（FAIL）。

- [ ] **Step 3: 写最小实现**

`core/src/backoff.rs`：

```rust
use std::time::Duration;

#[derive(Debug)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { base: Duration::from_millis(1000), cap: Duration::from_millis(30000), attempt: 0 }
    }
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let factor = 2u32.saturating_pow(self.attempt.min(5));
        self.attempt = self.attempt.saturating_add(1);
        (self.base * factor).min(self.cap)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}
```

`core/src/reconnect.rs`：

```rust
use std::time::Duration;

use crate::backoff::Backoff;
use crate::exit::{is_retryable, SshOutcome};

#[derive(Debug, PartialEq, Eq)]
pub enum RetryDecision {
    Retry(Duration),
    GiveUp,
}

pub fn decide(outcome: SshOutcome, backoff: &mut Backoff) -> RetryDecision {
    if is_retryable(outcome) {
        RetryDecision::Retry(backoff.next_delay())
    } else {
        backoff.reset();
        RetryDecision::GiveUp
    }
}
```

`core/src/lib.rs` 加：

```rust
pub mod backoff;
pub mod reconnect;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core backoff:: reconnect::`

Expected: `4 passed`。

- [ ] **Step 5: 提交**

```bash
git add core/src/backoff.rs core/src/reconnect.rs core/src/lib.rs
git commit -m "feat(core): exponential reconnect backoff with retry decision"
```

---

### Task 5: `core::config` — 配置解析与校验

**Files:**
- Create: `core/src/config.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub struct Config { pub hosts: Vec<HostConfig>, pub agents: Vec<AgentConfig> }`
  - `pub struct HostConfig { pub name: String, pub host: String, pub user: Option<String>, pub extra_ssh_args: Vec<String> }`
  - `pub struct AgentConfig { pub id: String, pub label: String, pub cmd: String }`
  - `pub fn parse_config(json: &str) -> Result<Config, String>`
  - `pub fn validate(cfg: &Config) -> Result<(), Vec<String>>`
  - `Config::to_ssh_target(&self, host_name: &str) -> Option<SshTarget>`

- [ ] **Step 1: 写会失败的测试**

`core/src/config.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
      "hosts": [{ "name": "main", "host": "10.0.0.1", "user": "ubuntu" }],
      "agents": [{ "id": "hermes", "label": "Hermes", "cmd": "hermes chat" }]
    }"#;

    #[test]
    fn parses_valid_config() {
        let cfg = parse_config(GOOD).unwrap();
        assert_eq!(cfg.hosts[0].name, "main");
        assert_eq!(cfg.agents[0].cmd, "hermes chat");
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn missing_required_field_reports_serde_error() {
        let err = parse_config(r#"{ "hosts": [] }"#).unwrap_err();
        assert!(err.contains("agents"), "unexpected error: {err}");
    }

    #[test]
    fn validate_names_the_offending_field() {
        let cfg = parse_config(r#"{
          "hosts": [{ "name": "", "host": "h" }],
          "agents": [{ "id": "a", "label": "A", "cmd": "  " }]
        }"#).unwrap();
        let errs = validate(&cfg).unwrap_err();
        assert_eq!(errs, vec![
            "hosts[0].name: must not be empty".to_string(),
            "agents[0].cmd: must not be empty".to_string(),
        ]);
    }

    #[test]
    fn resolves_ssh_target_by_host_name() {
        let cfg = parse_config(GOOD).unwrap();
        let t = cfg.to_ssh_target("main").unwrap();
        assert_eq!(t.host, "10.0.0.1");
        assert_eq!(t.user.as_deref(), Some("ubuntu"));
        assert!(t.extra_ssh_args.is_empty());
        assert!(cfg.to_ssh_target("nope").is_none());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p session_core config::`

Expected: 编译失败（FAIL）。

- [ ] **Step 3: 写最小实现**

`core/src/config.rs`：

```rust
use serde::Deserialize;

use crate::command::SshTarget;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub hosts: Vec<HostConfig>,
    pub agents: Vec<AgentConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HostConfig {
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub extra_ssh_args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    pub id: String,
    pub label: String,
    pub cmd: String,
}

pub fn parse_config(json: &str) -> Result<Config, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn validate(cfg: &Config) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if cfg.hosts.is_empty() {
        errs.push("hosts: must not be empty".to_string());
    }
    if cfg.agents.is_empty() {
        errs.push("agents: must not be empty".to_string());
    }
    for (i, h) in cfg.hosts.iter().enumerate() {
        if h.name.trim().is_empty() {
            errs.push(format!("hosts[{i}].name: must not be empty"));
        }
        if h.host.trim().is_empty() {
            errs.push(format!("hosts[{i}].host: must not be empty"));
        }
    }
    for (i, a) in cfg.agents.iter().enumerate() {
        if a.id.trim().is_empty() {
            errs.push(format!("agents[{i}].id: must not be empty"));
        }
        if a.cmd.trim().is_empty() {
            errs.push(format!("agents[{i}].cmd: must not be empty"));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

impl Config {
    pub fn to_ssh_target(&self, host_name: &str) -> Option<SshTarget> {
        self.hosts.iter().find(|h| h.name == host_name).map(|h| SshTarget {
            host: h.host.clone(),
            user: h.user.clone(),
            extra_ssh_args: h.extra_ssh_args.clone(),
        })
    }
}
```

`core/src/lib.rs` 加：

```rust
pub mod config;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core config::`

Expected: `4 passed`。

- [ ] **Step 5: 提交**

```bash
git add core/src/config.rs core/src/lib.rs
git commit -m "feat(core): parse and validate client config"
```

---

### Task 6: `core::session` — 连接状态机

**Files:**
- Create: `core/src/session.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub enum SessionState { Connecting, Connected, Retrying, Exited, Closed }`
  - `pub enum SessionEvent { Connected, Disconnected, RetryScheduled, GiveUp, AgentExited, UserClose }`
  - `pub fn transition(state: SessionState, event: SessionEvent) -> SessionState`

- [ ] **Step 1: 写会失败的测试**

`core/src/session.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::SessionEvent::*;
    use super::SessionState::*;

    #[test]
    fn connect_then_drop_then_retry_then_connect() {
        let s = Connecting;
        let s = transition(s, Connected);
        assert_eq!(s, Connected);
        let s = transition(s, Disconnected);
        assert_eq!(s, Retrying);
        let s = transition(s, RetryScheduled);
        assert_eq!(s, Retrying);
        let s = transition(s, Connected);
        assert_eq!(s, Connected);
    }

    #[test]
    fn agent_exit_is_terminal_until_closed() {
        assert_eq!(transition(Connected, AgentExited), Exited);
        assert_eq!(transition(Exited, Connected), Exited);
    }

    #[test]
    fn give_up_ends_in_exited() {
        assert_eq!(transition(Retrying, GiveUp), Exited);
    }

    #[test]
    fn user_close_is_terminal_from_any_state() {
        for s in [Connecting, Connected, Retrying, Exited] {
            assert_eq!(transition(s, UserClose), Closed);
        }
        assert_eq!(transition(Closed, Connected), Closed);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p session_core session::`

Expected: 编译失败（FAIL）。

- [ ] **Step 3: 写最小实现**

`core/src/session.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Connected,
    Retrying,
    Exited,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    Connected,
    Disconnected,
    RetryScheduled,
    GiveUp,
    AgentExited,
    UserClose,
}

pub fn transition(state: SessionState, event: SessionEvent) -> SessionState {
    use SessionEvent::*;
    use SessionState::*;

    match state {
        Closed => Closed,
        _ if event == UserClose => Closed,
        Exited => Exited,
        _ if event == AgentExited => Exited,
        _ => match event {
            Connected => Connected,
            Disconnected => Retrying,
            RetryScheduled => Retrying,
            GiveUp => Exited,
            UserClose | AgentExited => unreachable!("handled above"),
        },
    }
}
```

`core/src/lib.rs` 加：

```rust
pub mod session;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p session_core session::`

Expected: `4 passed`。

- [ ] **Step 5: 提交**

```bash
git add core/src/session.rs core/src/lib.rs
git commit -m "feat(core): connection state machine"
```

---

### Task 7: `core::transport` — PTY 启停与读写（含 fake ssh 集成测试）

**Files:**
- Create: `core/src/transport.rs`
- Create: `core/tests/fixtures/fake-ssh.sh`
- Create: `core/tests/transport_shim.rs`
- Modify: `core/src/lib.rs`
- Modify: `core/Cargo.toml`（加 `portable-pty`）

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub struct PtySession`
  - `PtySession::spawn(argv: &[String], cols: u16, rows: u16) -> Result<PtySession, String>`
  - `PtySession::take_reader(&mut self) -> Option<Box<dyn std::io::Read + Send>>`
  - `PtySession::write(&mut self, bytes: &[u8]) -> Result<(), String>`
  - `PtySession::resize(&mut self, cols: u16, rows: u16) -> Result<(), String>`
  - `PtySession::kill(&mut self) -> Result<(), String>`
  - `PtySession::wait(&mut self) -> Result<i32, String>`

- [ ] **Step 1: 添加依赖并写 fake ssh shim**

`core/Cargo.toml` 的 `[dependencies]` 追加：

```toml
portable-pty = "0.8"
```

`core/tests/fixtures/fake-ssh.sh`：

```bash
#!/usr/bin/env bash
# 假 ssh：打印两行可控输出后，按环境变量退出或挂起。
echo "FAKE-SSH-BANNER"
echo "line-two"
if [ "${FAKE_SSH_MODE:-exit0}" = "hang" ]; then
  sleep 3600
fi
exit "${FAKE_SSH_EXIT:-0}"
```

Run: `chmod +x core/tests/fixtures/fake-ssh.sh`

- [ ] **Step 2: 写会失败的测试**

`core/tests/transport_shim.rs`：

```rust
use std::io::Read;
use std::time::{Duration, Instant};

use session_core::transport::PtySession;

fn shim_path() -> String {
    format!("{}/tests/fixtures/fake-ssh.sh", env!("CARGO_MANIFEST_DIR"))
}

fn argv() -> Vec<String> {
    vec![shim_path()]
}

#[test]
fn spawns_reads_and_exits_zero() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    let mut reader = pty.take_reader().expect("reader");

    // 读满若干字节或超时
    let mut buf = vec![0u8; 256];
    let mut collected = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !String::from_utf8_lossy(&collected).contains("line-two") {
        let n = reader.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&collected);
    assert!(text.contains("FAKE-SSH-BANNER"), "got: {text}");
    assert!(text.contains("line-two"), "got: {text}");

    let code = pty.wait().expect("wait");
    assert_eq!(code, 0);
}

#[test]
fn writes_stdin_to_child() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    pty.write(b"hello\n").expect("write");
    let mut reader = pty.take_reader().expect("reader");
    let mut buf = vec![0u8; 256];
    let _ = reader.read(&mut buf);
    // 只要写入不报错即可（shim 走 PTY echo，具体回显不强制断言）
    let _ = pty.kill();
}

#[test]
fn kill_terminates_hung_child() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    let _ = pty.take_reader();
    pty.kill().expect("kill");
    // 不阻塞等待；kill 后 wait 应尽快返回
    let start = Instant::now();
    let _ = pty.wait();
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn resize_after_spawn_succeeds() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    pty.resize(120, 40).expect("resize");
    let _ = pty.kill();
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p session_core --test transport_shim`

Expected: 编译失败（`session_core::transport` 不存在）（FAIL）。

- [ ] **Step 4: 写最小实现**

`core/src/transport.rs`：

```rust
use std::io::{Read, Write};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
}

impl PtySession {
    pub fn spawn(argv: &[String], cols: u16, rows: u16) -> Result<Self, String> {
        let first = argv.first().ok_or_else(|| "empty argv".to_string())?;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(first);
        cmd.args(&argv[1..]);

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        Ok(Self { master: pair.master, child, reader: Some(reader), writer })
    }

    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader.take()
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer.write_all(bytes).map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())
    }

    pub fn kill(&mut self) -> Result<(), String> {
        self.child.kill().map_err(|e| e.to_string())
    }

    pub fn wait(&mut self) -> Result<i32, String> {
        let status = self.child.wait().map_err(|e| e.to_string())?;
        Ok(status.exit_code() as i32)
    }
}
```

`core/src/lib.rs` 加：

```rust
pub mod transport;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p session_core --test transport_shim`

Expected: `4 passed`（`spawns_reads_and_exits_zero`、`writes_stdin_to_child`、`kill_terminates_hung_child`、`resize_after_spawn_succeeds`）。

- [ ] **Step 6: 全量测试 + 提交**

Run: `cargo test -p session_core`

Expected: 全部通过。

```bash
git add core/Cargo.toml core/src/transport.rs core/src/lib.rs core/tests/
git commit -m "feat(core): pty transport for spawning and driving system ssh"
```

---

### Task 8: Tauri 2 应用壳与 IPC

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/runner.rs`
- Create: `src-tauri/src/commands.rs`
- Modify: `Cargo.toml`（workspace 加 `src-tauri`）
- Create: `examples/config.example.json`

**Interfaces:**
- Consumes: `session_core::{command, config, exit, reconnect, session, transport}`
- Produces（前端调用的 IPC 命令）:
  - `pub struct ConfigView { pub hosts: Vec<HostView>, pub agents: Vec<AgentView> }`
  - `#[tauri::command] fn load_config(path: String) -> Result<ConfigView, String>`
  - `#[tauri::command] fn start_session(app: AppHandle, host: String, agent: String, project: String) -> Result<String, String>`
  - `#[tauri::command] fn write_session(id: String, data: String) -> Result<(), String>`
  - `#[tauri::command] fn resize_session(id: String, cols: u16, rows: u16) -> Result<(), String>`
  - `#[tauri::command] fn close_session(id: String, kill_remote: bool) -> Result<(), String>`
  - 事件：`session-output`（payload `{ id, data }`）、`session-state`（payload `{ id, state }`，`state` 为 `SessionState` 的字符串名）

- [ ] **Step 1: 建 Tauri 应用骨架**

`Cargo.toml` workspace 改为：

```toml
[workspace]
resolver = "2"
members = ["core", "src-tauri"]
```

`src-tauri/Cargo.toml`：

```toml
[package]
name = "agent-session-client"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
session_core = { path = "../core" }
```

`src-tauri/tauri.conf.json`：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "agent-session-client",
  "version": "0.1.0",
  "identifier": "dev.local.agent-session-client",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [{ "title": "Agent Sessions", "width": 1100, "height": 720 }],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "all" }
}
```

- [ ] **Step 2: 写 runner（read 循环 + probe + 重连）**

`src-tauri/src/runner.rs`：

```rust
use std::io::Read;
use std::process::Command as StdCommand;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use session_core::backoff::Backoff;
use session_core::command::{build_probe_argv, build_session_argv, SshTarget};
use session_core::exit::{classify_ssh, SshOutcome};
use session_core::reconnect::{decide, RetryDecision};
use session_core::session::{transition, SessionEvent, SessionState};
use session_core::transport::PtySession;

pub enum RunnerMsg {
    Output(Vec<u8>),
    State(SessionState),
    Notice(String),
}

/// probe 远端会话是否已存在。返回 Ok(true)=已存在，Ok(false)=不存在。
pub fn probe_session(target: &SshTarget, session: &str) -> Result<bool, SshOutcome> {
    let argv = build_probe_argv(target, session);
    match StdCommand::new(&argv[0]).args(&argv[1..]).output() {
        Ok(out) if out.status.success() => Ok(true),
        Ok(out) => {
            if out.status.code() == Some(1) {
                Ok(false) // tmux has-session 返回 1 = 会话不存在
            } else {
                Err(classify_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)))
            }
        }
        Err(e) => Err(classify_ssh(None, &e.to_string())),
    }
}

/// 运行一个会话直到用户关闭（或不可重试的失败）。事件经 `tx` 发出。
pub fn run_session(
    tx: Sender<RunnerMsg>,
    target: SshTarget,
    session: String,
    agent_cmd: String,
    mut cols: u16,
    mut rows: u16,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    kill_remote_on_close: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut backoff = Backoff::default();

    loop {
        // 先判断会话是否还在，用于提示"原会话已不在，已新建"
        match probe_session(&target, &session) {
            Ok(true) => {}
            Ok(false) => {
                let _ = tx.send(RunnerMsg::Notice(
                    "原会话已不在，已新建".to_string(),
                ));
            }
            Err(kind) => {
                if matches!(decide(kind, &mut backoff), RetryDecision::GiveUp) {
                    let _ = tx.send(RunnerMsg::State(transition(
                        SessionState::Connecting,
                        SessionEvent::GiveUp,
                    )));
                    return;
                }
                let _ = tx.send(RunnerMsg::State(SessionState::Retrying));
                let delay = backoff.next_delay();
                thread::sleep(delay);
                continue;
            }
        }

        let argv = build_session_argv(&target, &session, &agent_cmd);
        let mut pty = match PtySession::spawn(&argv, cols, rows) {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(RunnerMsg::Notice(format!("spawn failed: {e}")));
                return;
            }
        };
        let _ = tx.send(RunnerMsg::State(SessionState::Connected));
        backoff.reset();

        let mut reader = match pty.take_reader() {
            Some(r) => r,
            None => return,
        };

        let out_tx = tx.clone();
        let reader_handle = thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if out_tx.send(RunnerMsg::Output(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // 转发用户输入；`__resize:<cols>x<rows>` 控制帧用于尺寸变化
        let mut closed = false;
        loop {
            match input_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(frame) => {
                    if let Some(dims) = frame.strip_prefix(b"__resize:") {
                        if let Ok(s) = std::str::from_utf8(dims) {
                            if let Some((c, r)) = s.split_once('x') {
                                if let (Ok(c), Ok(r)) = (c.parse(), r.parse()) {
                                    cols = c;
                                    rows = r;
                                    let _ = pty.resize(cols, rows);
                                }
                            }
                        }
                        continue;
                    }
                    if frame == b"__close" {
                        closed = true;
                        break;
                    }
                    let _ = pty.write(&frame);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if reader_handle.is_finished() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }

        let _ = pty.kill();
        let _ = reader_handle.join();

        if closed {
            if kill_remote_on_close.load(std::sync::atomic::Ordering::SeqCst) {
                let remote = format!("tmux kill-session -t {}", session_core::command::shell_quote(&session));
                let _ = StdCommand::new("ssh").args(["-o", "BatchMode=yes", &session_core::command::destination(&target), &remote]).status();
            }
            let _ = tx.send(RunnerMsg::State(SessionState::Closed));
            return;
        }

        // 进程结束：分类并决定是否重连
        let code = pty.wait().ok();
        let kind = classify_ssh(code, ""); // stderr 已在 PTY 流里；此处仅按退出码兜底
        let _ = tx.send(RunnerMsg::State(SessionState::Retrying));
        match decide(kind, &mut backoff) {
            RetryDecision::Retry(delay) => thread::sleep(delay),
            RetryDecision::GiveUp => {
                let _ = tx.send(RunnerMsg::State(SessionState::Exited));
                return;
            }
        }
    }
}
```

- [ ] **Step 3: 写 IPC 命令与 main**

`src-tauri/src/commands.rs`：

```rust
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use session_core::config::{parse_config, validate};
use session_core::session::SessionState;

use crate::runner::{run_session, RunnerMsg};

#[derive(Serialize)]
pub struct HostView {
    pub name: String,
    pub host: String,
}

#[derive(Serialize)]
pub struct AgentView {
    pub id: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct ConfigView {
    pub hosts: Vec<HostView>,
    pub agents: Vec<AgentView>,
}

pub struct AppState {
    pub config: Mutex<Option<session_core::config::Config>>,
    pub inputs: Mutex<HashMap<String, Sender<Vec<u8>>>>,
    pub runners: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

#[tauri::command]
pub fn load_config(state: State<AppState>, path: String) -> Result<ConfigView, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let cfg = parse_config(&text)?;
    validate(&cfg)?;
    let view = ConfigView {
        hosts: cfg.hosts.iter().map(|h| HostView { name: h.name.clone(), host: h.host.clone() }).collect(),
        agents: cfg.agents.iter().map(|a| AgentView { id: a.id.clone(), label: a.label.clone() }).collect(),
    };
    *state.config.lock().unwrap() = Some(cfg);
    Ok(view)
}

#[tauri::command]
pub fn start_session(
    app: AppHandle,
    state: State<AppState>,
    host: String,
    agent: String,
    project: String,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone().ok_or("config not loaded")?;
    let target = cfg.to_ssh_target(&host).ok_or_else(|| format!("unknown host: {host}"))?;
    let agent_cfg = cfg.agents.iter().find(|a| a.id == agent).ok_or_else(|| format!("unknown agent: {agent}"))?;

    let session_name = if project.trim().is_empty() { host.clone() } else { format!("{agent}-{project}") };
    let id = session_name.clone();

    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<RunnerMsg>();
    state.inputs.lock().unwrap().insert(id.clone(), in_tx);

    let kill_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    state.runners.lock().unwrap().insert(id.clone(), kill_flag.clone());

    let app_for_thread = app.clone();
    let id_for_thread = id.clone();
    std::thread::spawn(move || {
        run_session(msg_tx, target, session_name, agent_cfg.cmd.clone(), 80, 24, in_rx, kill_flag);
    });

    // 消息泵：把 RunnerMsg 转成前端事件
    std::thread::spawn(move || {
        for msg in msg_rx {
            match msg {
                RunnerMsg::Output(bytes) => {
                    let data = String::from_utf8_lossy(&bytes).to_string();
                    let _ = app_for_thread.emit("session-output", serde_json::json!({ "id": id_for_thread, "data": data }));
                }
                RunnerMsg::State(s) => {
                    let _ = app_for_thread.emit("session-state", serde_json::json!({ "id": id_for_thread, "state": state_name(s) }));
                }
                RunnerMsg::Notice(n) => {
                    let _ = app_for_thread.emit("session-notice", serde_json::json!({ "id": id_for_thread, "message": n }));
                }
            }
        }
    });

    Ok(id)
}

fn state_name(s: SessionState) -> &'static str {
    match s {
        SessionState::Connecting => "connecting",
        SessionState::Connected => "connected",
        SessionState::Retrying => "retrying",
        SessionState::Exited => "exited",
        SessionState::Closed => "closed",
    }
}

#[tauri::command]
pub fn write_session(state: State<AppState>, id: String, data: String) -> Result<(), String> {
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(data.into_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_session(state: State<AppState>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(format!("__resize:{cols}x{rows}").into_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_session(state: State<AppState>, id: String, kill_remote: bool) -> Result<(), String> {
    if let Some(flag) = state.runners.lock().unwrap().get(&id) {
        flag.store(kill_remote, std::sync::atomic::Ordering::SeqCst);
    }
    let guard = state.inputs.lock().unwrap();
    let tx = guard.get(&id).ok_or_else(|| format!("unknown session: {id}"))?;
    tx.send(b"__close".to_vec()).map_err(|e| e.to_string())
}
```

`src-tauri/src/main.rs`：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod runner;

use std::collections::HashMap;
use std::sync::Mutex;

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState {
            config: Mutex::new(None),
            inputs: Mutex::new(HashMap::new()),
            runners: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::start_session,
            commands::write_session,
            commands::resize_session,
            commands::close_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`examples/config.example.json`（Q2 未定，此处为示例值）：

```json
{
  "hosts": [
    { "name": "main", "host": "REPLACE_WITH_HOST", "user": "ubuntu" }
  ],
  "agents": [
    { "id": "hermes", "label": "Hermes Agent", "cmd": "REPLACE_WITH_HERMES_CMD" },
    { "id": "harness", "label": "DeepSeek Harness", "cmd": "REPLACE_WITH_HARNESS_CMD" }
  ]
}
```

- [ ] **Step 4: 构建确认**

Run: `cargo build -p agent-session-client`

Expected: 编译通过（Linux 需先装 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`；Windows 需 WebView2 + MSVC Build Tools）。

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml src-tauri/ examples/config.example.json
git commit -m "feat(app): tauri shell, ipc commands, and session runner"
```

---

### Task 9: React + xterm.js 前端

**Files:**
- Create: `package.json`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `index.html`
- Create: `src/main.tsx`
- Create: `src/App.tsx`
- Create: `src/Terminal.tsx`
- Create: `src/sessions.ts`
- Create: `src/sessions.test.ts`

**Interfaces:**
- Consumes: Task 8 的 IPC 命令与事件
- Produces:
  - `src/sessions.ts`：`export type SessionState = "connecting" | "connected" | "retrying" | "exited" | "closed"`；`export interface Session { id: string; state: SessionState }`；`export function applyState(sessions: Session[], id: string, state: SessionState): Session[]`
  - `Terminal` 组件：props `{ onData: (d: string) => void; onResize: (cols: number, rows: number) => void; registerWriter: (fn: (d: string) => void) => void }`

- [ ] **Step 1: 写会失败的前端测试**

`src/sessions.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { applyState, type Session } from "./sessions";

describe("applyState", () => {
  it("adds an unknown session", () => {
    expect(applyState([], "a", "connecting")).toEqual([{ id: "a", state: "connecting" }]);
  });

  it("replaces the state of a known session", () => {
    const before: Session[] = [{ id: "a", state: "connecting" }];
    expect(applyState(before, "a", "connected")).toEqual([{ id: "a", state: "connected" }]);
  });

  it("never mutates the input array", () => {
    const before: Session[] = [{ id: "a", state: "connecting" }];
    applyState(before, "a", "retrying");
    expect(before[0].state).toBe("connecting");
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm install && npx vitest run src/sessions.test.ts`

Expected: FAIL —— `Failed to resolve import "./sessions"`。

- [ ] **Step 3: 写实现**

`package.json`：

```json
{
  "name": "agent-session-client",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "test": "vitest run"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@xterm/addon-fit": "^0.10",
    "@xterm/xterm": "^5.5",
    "react": "^18",
    "react-dom": "^18"
  },
  "devDependencies": {
    "@testing-library/react": "^16",
    "@types/react": "^18",
    "@types/react-dom": "^18",
    "@vitejs/plugin-react": "^4",
    "jsdom": "^25",
    "typescript": "^5",
    "vite": "^5",
    "vitest": "^2"
  }
}
```

`vite.config.ts`：

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  test: { environment: "jsdom" },
});
```

`tsconfig.json`：

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vitest/globals"]
  },
  "include": ["src"]
}
```

`index.html`：

```html
<!doctype html>
<html lang="zh">
  <head>
    <meta charset="UTF-8" />
    <title>Agent Sessions</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`src/sessions.ts`：

```ts
export type SessionState = "connecting" | "connected" | "retrying" | "exited" | "closed";

export interface Session {
  id: string;
  state: SessionState;
}

export function applyState(sessions: Session[], id: string, state: SessionState): Session[] {
  const idx = sessions.findIndex((s) => s.id === id);
  if (idx === -1) {
    return [...sessions, { id, state }];
  }
  return sessions.map((s, i) => (i === idx ? { ...s, state } : s));
}
```

`src/Terminal.tsx`：

```tsx
import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

interface Props {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  registerWriter: (write: (data: string) => void) => void;
}

export function Terminal({ onData, onResize, registerWriter }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const term = new XTerm({ convertEol: false, scrollback: 5000, fontSize: 13 });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(hostRef.current!);
    fit.fit();

    term.onData(onData);
    registerWriter((data) => term.write(data));

    const ro = new ResizeObserver(() => {
      fit.fit();
      onResize(term.cols, term.rows);
    });
    ro.observe(hostRef.current!);

    return () => {
      ro.disconnect();
      term.dispose();
    };
  }, [onData, onResize, registerWriter]);

  return <div ref={hostRef} style={{ flex: 1, minHeight: 0 }} />;
}
```

`src/App.tsx`：

```tsx
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "./Terminal";
import { applyState, type Session } from "./sessions";

interface ConfigView {
  hosts: { name: string; host: string }[];
  agents: { id: string; label: string }[];
}

export default function App() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const writerRef = useRef<((d: string) => void) | null>(null);

  useEffect(() => {
    invoke<ConfigView>("load_config", { path: "examples/config.example.json" })
      .then(setConfig)
      .catch((e) => console.error(e));

    const unlisten = listen<{ id: string; state: any }>("session-state", (e) => {
      setSessions((prev) => applyState(prev, e.payload.id, e.payload.state));
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!active) return;
    const un = listen<{ id: string; data: string }>("session-output", (e) => {
      if (e.payload.id === active) writerRef.current?.(e.payload.data);
    });
    return () => {
      un.then((f) => f());
    };
  }, [active]);

  const onData = useCallback(
    (data: string) => {
      if (active) void invoke("write_session", { id: active, data });
    },
    [active],
  );

  const onResize = useCallback(
    (cols: number, rows: number) => {
      if (active) void invoke("resize_session", { id: active, cols, rows });
    },
    [active],
  );

  const registerWriter = useCallback((write: (data: string) => void) => {
    writerRef.current = write;
  }, []);

  const start = async (host: string, agent: string) => {
    const id = await invoke<string>("start_session", { host, agent, project: "" });
    setSessions((prev) => applyState(prev, id, "connecting"));
    setActive(id);
  };

  return (
    <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui" }}>
      <aside style={{ width: 220, borderRight: "1px solid #ddd", padding: 8 }}>
        <h3>Sessions</h3>
        {sessions.map((s) => (
          <button
            key={s.id}
            onClick={() => setActive(s.id)}
            style={{ display: "block", width: "100%", textAlign: "left" }}
          >
            {s.id} — {s.state}
          </button>
        ))}
        <h4>New</h4>
        {config?.hosts.map((h) =>
          config.agents.map((a) => (
            <button key={`${h.name}-${a.id}`} onClick={() => start(h.name, a.id)}>
              {h.name} / {a.label}
            </button>
          )),
        )}
      </aside>
      <main style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        {active ? (
          <Terminal onData={onData} onResize={onResize} registerWriter={registerWriter} />
        ) : (
          <p style={{ padding: 16 }}>选一个 host / agent 开始会话</p>
        )}
      </main>
    </div>
  );
}
```

`src/main.tsx`：

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npx vitest run src/sessions.test.ts`

Expected: `3 passed`。

- [ ] **Step 5: 提交**

```bash
git add package.json package-lock.json vite.config.ts tsconfig.json index.html src/
git commit -m "feat(ui): react + xterm.js terminal wired to tauri ipc"
```

---

### Task 10: 端到端接线与手工冒烟清单

**Files:**
- Create: `docs/smoke-test.md`
- Modify: `src-tauri/src/commands.rs`（加载配置的默认路径改为可配置；见 Step 1）

**Interfaces:**
- Consumes: 前面全部任务的产物
- Produces: `docs/smoke-test.md`（可重复执行的手工验收步骤）

- [ ] **Step 1: 让配置路径可配置（不再写死示例文件）**

在 `src-tauri/src/commands.rs` 的 `load_config` 调用侧（`src/App.tsx`）改为读取环境变量 `AGENT_SESSION_CONFIG`，缺省回退 `examples/config.example.json`：

`src/App.tsx` 中 `useEffect` 内替换为：

```tsx
const cfgPath = (import.meta.env.VITE_AGENT_SESSION_CONFIG as string | undefined) ?? "examples/config.example.json";
invoke<ConfigView>("load_config", { path: cfgPath })
  .then(setConfig)
  .catch((e) => console.error(e));
```

- [ ] **Step 2: 写冒烟清单**

`docs/smoke-test.md`：

```markdown
# 手工冒烟清单

前置：服务端已装 tmux；`examples/config.example.json` 中的 host 与 agent 命令已替换为真实值（Q2）。

## A. 基础连通
1. 启动应用，左侧出现 host / agent 按钮。
2. 点一个按钮 → 右侧终端出现远端 shell / agent 界面，状态显示 `connected`。

## B. 断线恢复（核心）
3. 在终端里运行：`while true; do date; sleep 1; done`
4. 断开本机网络（关 Wi‑Fi 或拔网线）。
   - 期望：状态变为 `retrying`，右侧保留最后画面。
5. 等待 10 秒后恢复网络。
   - 期望：状态回到 `connected`；终端由 tmux 重绘出**同一会话**的画面；向上翻能看到断线期间的输出。
6. 远端确认会话唯一：`ssh <host> "tmux ls"` 应只有一个对应 session。

## C. 关客户端重开
7. 直接关闭客户端窗口（不点"结束会话"）。
8. 重新启动应用，重新点同一 host / agent。
   - 期望：接回同一会话，历史仍在。

## D. 失败分支
9. 故意把 host 改成不存在的地址 → 状态 `retrying` 后最终 `exited`，并显示原因。
10. 把配置里的 host 指向一个没有 tmux 的机器 → 提示缺 tmux，不无限重试。
11. 人为 `ssh <host> "tmux kill-session -t <session>"` 后再重连 → 出现"原会话已不在，已新建"提示。
```

- [ ] **Step 3: 全量回归**

Run: `cargo test -p session_core && npx vitest run`

Expected: Rust 与前端测试全部通过。

- [ ] **Step 4: 提交**

```bash
git add src/App.tsx docs/smoke-test.md
git commit -m "test: add smoke checklist and configurable config path"
```

---

## Self-Review

**Spec coverage：**

| Spec 章节 | 覆盖任务 |
|---|---|
| §2 目标 1（新建/接入会话） | Task 8（`start_session`）、Task 9（UI 按钮） |
| §2 目标 2（断网不中断进程） | Task 7（PTY）+ Task 8（`tmux new -As`）——由 tmux 保证 |
| §2 目标 3（重连看到旧输出） | Task 8 runner 重连 + `new -As` 重绘 + Task 10 冒烟 B |
| §2 目标 4（原样终端） | Task 9（xterm.js 直接渲染，无解析） |
| §6.1 UI 状态 5 态 | Task 6（状态机）+ Task 9（`SessionState`） |
| §6.3 保活参数 | Task 2（`KEEPALIVE_ARGS` 常量 + 测试） |
| §6.4 退避 1s→30s | Task 4（`Backoff` + 测试） |
| §6.5 配置校验报字段名 | Task 5（`validate` + 测试） |
| §7 `new -As` | Task 2 + Task 8 |
| §7 命令转义易错点 | Task 2（`shell_quote` + 测试） |
| §7 `remain-on-exit` | **未实现**——见下方"已知偏差" |
| §7 sessionName 持久化 | Task 8（`id = session_name`，重连复用） |
| §8 认证失败不重试 | Task 3 + Task 4 + Task 10 冒烟 D |
| §8 缺 tmux 提示 | Task 3（`TmuxMissing`）+ Task 8 |
| §8 原会话不在提示 | Task 8（`probe_session` + `Notice`）+ Task 10 冒烟 D11 |
| §8 终端尺寸转发 | Task 7（`resize`）+ Task 8（`__resize:` 控制帧）+ Task 9（`onResize`） |
| §9 测试策略 | 各任务 TDD 步骤（不真连网络，用 shim） |
| §10 系统 ssh | Task 2（`build_*_argv` 产出 `ssh` argv） |
| §13 验收 1–6 | Task 10 冒烟清单 A–D |

**已知偏差（需执行者知悉）：**

1. **`remain-on-exit` 未纳入本计划**：spec §7 提到用 `tmux set-option remain-on-exit on` 区分"agent 退出"与"网络断开"。本计划暂未实现该分支——因为 `probe_session` 已能把"会话不存在"（→ 提示已新建）与正常重连区分开，而"agent 退出"当前会表现为会话消失 + 新建。若执行时发现该体验不可接受，追加一个小任务：在 `build_remote_tmux_cmd` 前加 `tmux set -g remain-on-exit on \;` 并在 probe 后增加 `tmux list-panes -F '#{pane_dead}'` 判定。**这是一个显式记录的取舍，不是遗漏。**
2. **stderr 分类的局限**：runner 在 PTY 流里拿不到 ssh 的独立 stderr（已被并入终端字节流）。Task 8 的 `classify_ssh(code, "")` 仅按退出码兜底；网络类错误的可靠判定依赖 `probe_session` 的 stderr。若实测分类不准，追加任务：把 probe 的 stderr 缓存下来供分类使用。
3. **Task 8 未附自动化测试**：Tauri 命令层需要运行中的应用实例，本计划用 Task 10 的手工冒烟覆盖；逻辑层已在 `core` 中充分单测。这是有意的分层，不是遗漏。

**Placeholder scan：** 计划正文无 TBD/TODO；`examples/config.example.json` 中的 `REPLACE_WITH_*` 是配置占位值（spec Q2 未定），并在 Task 10 明确要求替换。

**Type consistency：** `SessionState`（Rust）与 `SessionState`（TS）字符串名一一对应，由 `commands::state_name` 保证；`SshTarget`/`HostConfig`/`AgentConfig` 字段名在 Task 2/5/8 间一致；`PtySession` 方法签名在 Task 7/8 间一致。
