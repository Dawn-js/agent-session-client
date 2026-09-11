# Windows 构建环境搭建清单

> **用途**：为 `agent-session-client` 的 **Task 8–10**（Tauri 2 应用壳、React/xterm.js 前端、端到端冒烟）准备 Windows 构建机。
> **对应**：`docs/superpowers/plans/2026-09-11-agent-session-client.md` 的 Task 8/9/10；spec §11 前置条件。
> **整理**：2026-09-11。核对人：阿七。

---

## 0. 范围与原则

- 目标只有三条：**能 `cargo build` 出 Tauri Windows 应用**、**能跑 Vite/React 前端**、**能 `cargo test`**。
- **不需要 WSL**——Tauri 在 Windows 上原生构建（MSVC 工具链）。
- 能用 `winget` 就 `winget`（管理员 PowerShell）。
- 现在只装环境，**不建代码骨架**——代码由 Task 8/9 的执行产生。

## 1. 系统要求

- [ ] Windows 10 **1803+** 或 Windows 11（WebView2 随系统附带；1803 以下需手动装 Runtime）
- [ ] 确认架构（决定 Rust host triple）：

  ```powershell
  echo $env:PROCESSOR_ARCHITECTURE   # AMD64 或 ARM64
  ```

- [ ] 磁盘剩余 ≥ **10 GB**（VS Build Tools ≈ 6 GB + Rust ≈ 1.5 GB + node_modules）
- [ ] 管理员权限（仅安装 Build Tools 时需要）

## 2. 安装（按顺序）

### 2.1 Git for Windows

```powershell
winget install --id Git.Git -e
```

- [ ] 装完新开终端，`git --version` 有输出

### 2.2 Visual Studio 2022 Build Tools（C++ 工作负载 + Windows SDK）——Tauri/MSVC 硬依赖

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

`--includeRecommended` 会带上 MSVC v143 编译器与 Windows 11 SDK，正是链接 Tauri 所需。

- [ ] 装完**新开终端**（PATH 不刷新是"找不到 link.exe"的头号原因）

### 2.3 Rust（MSVC host）

```powershell
winget install --id Rustlang.Rustup -e
rustup default stable
```

- [ ] `rustc -vV` 的 `host:` 应为 `x86_64-pc-windows-msvc`（ARM64 机器则为 `aarch64-pc-windows-msvc`）
- [ ] **必须**是 `*-msvc`，不要 `*-gnu`（本项目按 MSVC 构建）

### 2.4 Node.js LTS（≥ 20）

```powershell
winget install --id OpenJS.NodeJS.LTS -e
```

- [ ] `node -v` / `npm -v` 有输出

### 2.5 WebView2 Runtime

Win10 1803+/Win11 通常已预装。缺失时：

```powershell
winget install --id Microsoft.EdgeWebView2Runtime -e
```

### 2.6 （可选）编辑器

- VS Code + `rust-analyzer`、`Tauri`、`EditorConfig` 扩展

## 3. 仓库与认证

- [ ] 长路径支持（Rust 依赖目录很深，防 `MAX_PATH` 报错）：

  ```powershell
  git config --global core.longpaths true
  ```

- [ ] 克隆：

  ```powershell
  git clone https://github.com/Dawn-js/agent-session-client.git
  cd agent-session-client
  git checkout feat/core   # Task 8/9 在这个分支上继续
  ```

- [ ] **认证（重要）**：`feat/core` 是私有仓库分支。**不要复用 2026-09-11 那个已在聊天记录中泄露的 token（必须吊销）**。二选一：
  - **fine-grained PAT**：只授权 `agent-session-client` 这一个仓库 + `Contents: Read and write`，有效期设 90 天；
  - **SSH**：`ssh-keygen -t ed25519`，公钥加到 <https://github.com/settings/keys>，`git clone git@github.com:Dawn-js/agent-session-client.git`。

## 4. 装完立即验证（别等 Task 8）

```powershell
git --version; node -v; npm -v; cargo -V; rustc -V

cd agent-session-client
cargo test -p session_core
```

- [ ] **期望：`28 passed; 0 failed`（24 unit + 4 transport_shim），零警告。**
  这一步在 Windows 上全绿，就证明 Rust + MSVC 链路已通——是 Task 8 开工前的环境验收，不依赖任何前端/Tauri 代码。
- 首次构建会被 Windows Defender 拖慢；可将仓库目录加入 Defender 排除项。

## 5. Task 8/9 落地时才需要（现在不做）

- `npm install` —— `package.json` 由 Task 9 产生。
- **Tauri CLI 走 npm 本地依赖**（`@tauri-apps/cli` devDependency，`npx tauri …`），**不全局安装** `tauri-cli` —— 少一个全局状态，版本跟仓库走。
- **不需要** `webkit2gtk`——那是 Linux 构建路径的依赖。

## 6. 常见坑

| 症状 | 原因 / 解法 |
|---|---|
| `link.exe not found` | Build Tools 的 C++ 工作负载没装，或装完没开新终端 |
| Rust host 是 `*-gnu` | 重装 rustup 时默认勾了 GNU；`rustup default stable-msvc` 纠正 |
| 路径超长报错 | `core.longpaths true`（见 §3） |
| npm 脚本被拦 | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` |
| ARM64 机器上构建失败 | `rustup target add aarch64-pc-windows-msvc`；WebView2 有 ARM64 版 |
| 疑似 node-gyp 报错 | 本项目前端无原生依赖，不应出现——先怀疑 Node 版本过旧 |

## 7. 一键自检脚本

保存为 `check-env.ps1`，缺什么一目了然：

```powershell
$ErrorActionPreference = "Continue"
$fail = $false
foreach ($c in "git", "node", "npm", "cargo", "rustc") {
    $v = & $c --version 2>$null | Select-Object -First 1
    if ($v) { Write-Host "OK      $c -> $v" }
    else    { Write-Host "MISSING $c"; $fail = $true }
}
$hostTriple = (& rustc -vV 2>$null | Select-String "^host:").Line
Write-Host "rust host: $hostTriple"
if ($hostTriple -notmatch "-msvc") { Write-Host "WARN: host is not MSVC"; $fail = $true }

# WebView2 Runtime（机器级注册表；部分机器是用户级安装，MISSING 不一定真缺）
$key = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$wv  = Get-ItemProperty $key -ErrorAction SilentlyContinue
if ($wv) { Write-Host "OK      WebView2 -> $($wv.pv)" }
else     { Write-Host "CHECK   WebView2 (not machine-wide; run §2.5 if §4 fails)" }

if ($fail) { Write-Host "`nENV INCOMPLETE" -ForegroundColor Red; exit 1 }
Write-Host "`nENV OK" -ForegroundColor Green
```

---

*对应服务端侧已完成的工作见 `docs/superpowers/plans/2026-09-11-agent-session-client.md` 与 PR #1 的描述；Task 8 派发时的 5 条 carry-forward 契约见该 PR 的 "Carry-forward contracts" 一节。*
