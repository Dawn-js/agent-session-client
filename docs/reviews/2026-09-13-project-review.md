# 项目评审：agent-session-client

日期：2026-09-13 ｜ 版本：0.2.5 ｜ 提交：83a16fb
评审方式：全项目（非 diff），按 `code-review-and-quality` 五轴

---

## 结论

**Approve with required changes** —— 代码健康度明显高于同类个人项目。
主要缺口两个：`src-tauri/` 没有测试、滚轮问题的根因未查清（只绕过了）。

---

## 规模与结构

| 部分 | 行数 | 最大文件 |
|---|---|---|
| `core/`（纯逻辑，可测） | 1845 | `command.rs` 373 |
| `src-tauri/`（集成层） | 1019 | `commands.rs` 574 |
| `src/`（React + TS） | 2014 | `App.tsx` 551 |

分层是干净的：`core/` 不依赖 Tauri、不碰进程，全是纯函数 + 状态机；
`src-tauri/` 只做胶水（进程、IPC、窗口）；`src/` 只做呈现。
**依赖方向正确，没有循环依赖。**

---

## Correctness

**没有发现 correctness 缺陷。** 重点核查了几处易错的地方：

- `runner.rs` 的每条退出路径都做了 `pty.kill()` + `wait()`（`PtySession` 没有 `Drop`，
  漏一条就留僵尸进程）。三条路径（正常关闭、意外退出、读取失败）都覆盖了。
- `filechan.rs` 的超时处理看似会留下"脏响应"，但调用方 `commands.rs::with_chan`
  在失败时会 `take()` 掉整条通道并重建，脏响应随旧 channel 一起被丢弃 —— 兜住了。
- 今天修掉的 copy-mode 卡死（滚动命令缺 `-e`）**是真实的 correctness 缺陷**，
  已加断言锁住。

## Readability & Simplicity

**这是项目最强的部分。** 注释几乎都在解释**为什么**而不是**是什么**，例如
`command.rs` 里 `login_shell` 的长注释直接写清了"非交互 ssh 的 PATH 不含 `~/.local/bin`，
而 tmux 新建会话继承的是客户端环境"—— 这类注释在同类项目里很罕见。

`AGENTS.md` 的踩坑 ledger（20 条）是**超出常规的实践**，把"为什么不能那样做"
固化成了可检索的知识，直接降低了后续会话的返工率。

**Nit**：`filechan.rs:27` 的注释「超时后响应流是脏的，整个通道会被丢弃重建」
描述的是**调用方**（`with_chan`）的行为，放在 `filechan.rs` 的常量注释里，
读者会误以为 `FileChan` 自己会重建。建议移到 `with_chan` 或补一句"由调用方负责"。

## Architecture

分层与边界都站得住。三点观察：

**Optional**：`App.tsx` 551 行，超过「组件 200 行」的建议线。它承担了
状态管理 + 布局编排 + 三块面板的渲染。可拆的是三个区块：
会话列表、新建会话、终端区（含头部/notice）。拆完主组件能回到 200 行内。

**Optional**：`hostOf` 与 `closedRef` 只增不减 —— 会话关闭后条目仍留在
`Record` / `Set` 里。因为 id 就是 agent 名（数量有限），实际影响是内存噪声而非泄漏，
不紧急。真要清，`closeSession` 里顺手删一下即可。

**FYI**：`commands.rs` 574 行里混了三类东西 —— 配置读写、会话生命周期、
远端命令执行。目前还能读，但如果再加功能，建议按职责拆成
`config_cmds.rs` / `session_cmds.rs` / `remote.rs`。

## Security

- 远端命令全部由程序构造，路径统一走 `shell_quote`（有专门测试覆盖含单引号的用例）✓
- `filechan.rs` 的 `eval "$line"` 只接受程序构造的命令，不接受外部输入 ✓
  但这条**依赖调用方纪律** —— 将来若有人把用户输入拼进去就是注入。
  建议在 `REMOTE_LOOP` 上方补一句警告。
- 无凭据落盘、无日志泄漏 ✓
- **拦截 `Ctrl+C/D/Z/\`**（`keys::strip_quit_keys`）是刻意的安全设计，
  防止误触杀掉 agent 及其 tmux 会话 ✓ 有测试。

## Performance

- 长驻通道把「列目录 3-5s」降到「一个 RTT」✓
- PTY 输出 8KB 缓冲 + UTF-8 边界拼装，没有逐字节走 IPC ✓
- 前端 `onScroll` 有 80ms 节流、resize 有 120ms 防抖 ✓
- **无 N+1、无无界循环** ✓

## Tests

| 模块 | 测试数 |
|---|---|
| `core/`（13 个文件） | **100** ✓ |
| `src-tauri/`（4 个文件，1019 行） | **0** ✗ |
| `src/`（前端） | 33 ✓ |

**Required**：`src-tauri/` 零测试，而其中至少有四处是**纯逻辑、完全可测**的：

1. `filechan.rs::read_loop` —— `__END__<code>` 协议解析（多行输出、跨块、脏尾）
2. `commands.rs::with_chan` —— 失败重建重试的语义
3. `runner.rs::probe_session` —— 退出码 1 与其它错误的区分（`Ok(false)` vs `Err`）
4. `runner.rs::finish_close` —— `kill_remote` 开关的分支

这些恰恰是**今天改动最多**的地方，却没有回归保护。抽成纯函数即可测，
不需要 Tauri 运行时。

---

## 需要处理（按优先级）

| 级别 | 事项 |
|---|---|
| **Required** | 给 `src-tauri/` 的四处纯逻辑补测试（尤其 `read_loop` 协议解析与 `with_chan` 重试） |
| **Required** | **查清「控制字符到不了 tmux」的根因** —— 见下 |
| Optional | 拆 `App.tsx` 的三个区块；清理 `hostOf`/`closedRef` 的残留条目 |
| Nit | `filechan.rs:27` 注释位置；`REMOTE_LOOP` 上方补"eval 只接受程序构造命令"的警告 |

## 关于滚轮：根因仍是未知

这是本次评审**最值得单独提出来的一条**。

现象：用户环境里 `Ctrl+b` 这类控制字符**到不了 tmux**。而 tmux 侧实测完全正常
（pty 客户端发 `\x02[` 时 `pane_in_mode=1`）。

处理方式：**绕过了它** —— 滚轮改由客户端直接下发
`tmux copy-mode -e -t <session>; tmux send-keys -t <session> -X -N 3 scroll-up`，
不再依赖按键链路。**功能是好的，但根因没查明。**

**为什么这不只是个"已解决"的问题**：如果控制字符真的到不了 tmux，那么受影响的
**不只是滚轮** —— 所有需要控制字符的交互都在同一风险下（`Ctrl+b` 之外的快捷键、
`Esc` 序列、未来的 `Ctrl+Shift+*` 绑定）。现在只是**恰好**没有第二个功能依赖它。

**建议**：在 GUI 里做一次最小复现 —— 开一个普通 shell 会话，跑 `cat -v`，
按 `Ctrl+b` 看是否回显 `^B`。如果**不回显**，就沿
`xterm.onKey → onData → invoke → runner → PTY` 逐层打点，定位是
xterm 没发、还是 WebView2 拦了、还是 runner 丢了。这条查清了，
以后所有快捷键类需求都不用再绕。

（注：本次曾尝试用 Xvfb 跑真 GUI 复现，但 **XTest 的合成点击在 WebKitGTK 里不生效**，
按钮点不动，只能看静态画面 —— 所以这条得靠手工复现，或换一种事件注入方式。）

---

## 值得保留的做法

1. `core/` 与集成层分离 —— 让 100 个测试能脱离 Tauri 跑
2. 注释记录**决策理由**与**实测数据**（"实测 3-5s，其中传数据 0.8s"）
3. `AGENTS.md` ledger —— 把踩过的坑固化成后续会话可检索的知识
4. 依赖克制：前端只加了 4 个 `@xterm/*` 包，没有引入状态管理库
5. 每个修复都留了测试（今天两次滚轮修复都补了断言）
