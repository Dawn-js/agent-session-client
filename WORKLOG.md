# WORKLOG

> 倒序排列，最新在顶部。每次会话收工前更新（见 AGENTS.md「收工规矩」）。

## 2026-09-12（文件面板通道 / 滚轮 / 主题切换）

**本次做了什么**

- **文件面板复用一条 ssh**：新增 `src-tauri/src/filechan.rs`（长驻 ssh + 一行一命令的协议，
  远端 `eval` 后以独占一行的 `__END__<退出码>` 分帧），`list_dir` / `list_skills` 改走它。
  原先每点一次目录都要重走一遍 TCP+SSH 握手——实测端到端 3-5s，其中传数据只有 0.8s。
- **消除 cmd 弹窗**：`list_dir` / `list_skills` 也补上 `CREATE_NO_WINDOW`
  （原先只有 runner 里的 ssh 加了，这两个命令是后来才有的）。
- **滚轮**：`build_remote_tmux_cmd` 前置 `tmux set -g mouse on`。tmux 的 mouse 模式默认是关的，
  不开的话滚轮事件在 tmux 里不产生任何滚动。
- **深浅主题切换**：`<html data-theme>` + CSS 变量（组件里不再留硬编码颜色），
  xterm 配两套 ANSI 配色；换主题时只改 `term.options.theme`，不重建终端，避免丢掉整屏回滚。
  选择存 localStorage。
- **长驻通道的收尾（同一轮补的）**：应用退出时 kill 掉那条 ssh（进程退出不会替我们收子进程，
  不 kill 就是孤儿——ledger 第 2 条的同款问题）；`load_config` / `save_config` 换过配置后
  丢弃旧通道，让它按新的 target 重建。

**当前状态**

- `cargo test`（62）、`npm test`（14）、`npm run build`、`cargo check -p agent-session-client`
  全部在 `main` 服务器上跑绿（本机拉不到依赖，见下方踩坑）。
- **未经 Windows 目视确认**：文件面板提速体感、滚轮是否真能翻历史、浅色主题下终端可读性。

**下一步计划**

- Windows 上实测三件事：滚轮翻 tmux 历史、文件面板是否秒开、浅色主题观感。
- tmux mouse on 之后鼠标拖拽选择会被 tmux 接管（系统选区需按住 Shift）。
  不想要就在远端 `tmux set -g mouse off`，代价是滚轮又不能滚——二者只能选一个。

**踩过的坑**

- **ControlMaster 在这台机器上不工作**：master 能起来（`ssh -O check` 报 `Master running`），
  但每次 session 请求都 `mux_client_request_session: read from master failed: Connection reset by peer`
  后退回新建连接，三次耗时 2.4s / 3.3s / 3.0s，**与不复用完全一样**。
  `/tmp/...` 和 Windows Temp 两种 ControlPath 都试过。所以改成自建通道，而不是加三个 `-o`。
- **我把"本机跑不了"归错了因（已更正，见 ledger 第 14 条）**：先是看到 7890 不通就断定"本机拉不到依赖"，
  实际是客户端换了端口（现在 7897）、且 WorkBuddy 会话会注入 `http_proxy=127.0.0.1:50261` 覆盖掉设置。
  摘掉代理变量后本机 `npm install`（173 包）/ `npm run build` / `npm test` 全部正常。
  真正的限制只有一条：沙箱内 **curl 和 cargo（libcurl 系）的下载被截断**（HTTP 200 但 0 字节），
  所以 Rust 侧仍走 `main` 验证。**教训：只测一个端口就下结论，差点把错误结论写死在文档里。**
- 改 `build_remote_tmux_cmd` 时有**两个**测试断言了这条命令（`builds_remote_tmux_cmd` 和
  `builds_session_argv_with_keepalive_and_tty`），只改第一个会漏。
- Tauri 里 `app.state::<T>()` 是 **`Manager` trait 的方法**，要 `use tauri::Manager` 才能用，
  否则报 `no method named 'state' found for reference '&AppHandle'`（看起来像 API 不存在）。
  另外这个版本**没有 `try_state`**。事件循环要拿 managed state 就走 `app.state::<T>()`。
- 编辑文件后 `git add -A` 偶尔会认为"没有变化"（git status 干净但内容确实改了），
  此时 `git update-index --really-refresh` 刷新一下再 add。**提交后要确认 `git log -1`**，
  别信那句 `nothing to commit` —— 本次出现过它照着打印、提交其实成功的情况。

## 2026-09-12（文件面板 / TUI 修复 / 应用图标）

**本次做了什么**

- **服务器文件系统面板**（右侧）：core 新增 `files` 模块（`build_list_cmd` / `parse_listing` / `join_path`），
  `command` 新增 `build_exec_argv`（一次性 ssh），`src-tauri` 新增 `list_dir` 命令，前端新增 `FilePanel.tsx`。
  走一次性 ssh 进程，**不占用也不污染任何 PTY 会话**，未引入 SFTP 或任何新依赖。
- **拖拽插入路径**：文件/文件夹拖到终端即把远端绝对路径写入 PTY（走标准 `text/plain`）。
- **右键粘贴**：`onContextMenu` + `navigator.clipboard`，未引入剪贴板插件。
- **TUI 字体与对比度**：补齐 xterm 的 `fontFamily`（Cascadia Mono 栈）与完整 16 色 ANSI 调色板，
  另加 `fontWeightBold` / `lineHeight` / `letterSpacing`。
- **应用图标**：新增 `src-tauri/app-icon.svg`（两节点+连线，"会话/连接"主题），
  用 `npx tauri icon` 生成全平台尺寸，替换了原先的 Tauri 默认图标。

**当前状态**

- 版本 `0.2.0`，分支 `master`；`cargo test`（56）、`npm test`（14）、`npm run build` 全绿，
  `cargo check -p agent-session-client` 0 warning。
- 前端已构建通过，但**界面效果未经 Windows 目视确认**（本机编译不了 Rust，见下方「踩过的坑」）。

**下一步计划**

- 在 Windows 上 `npx tauri dev` 目视确认：文件面板布局、拖拽插入、右键粘贴、终端字体观感。
- 面板目前固定跟随当前会话的 host；如需「不开会话也能浏览」再加 host 选择器。
- 若要显示文件修改时间，`find -printf` 加回 `%T@` 即可（本次刻意未做，避免留无用字段）。

**踩过的坑**

- **本机 Windows 编译不了 Rust**：只装了 `x86_64-pc-windows-msvc` 工具链但没装 MSVC 链接器，
  且 `link.exe` 会被 Git Bash 的 coreutils `link` 遮蔽，报错是 `link: extra operand '...\*.rcgu.o'`，
  **看着像代码错，实际是工具链问题**。本机验证一律走 `main` 服务器（见 AGENTS.md「开发环境」）。
- **修正 AGENTS.md 的一处说法**：`cargo check -p agent-session-client` 在服务器上**能过**，
  并非"src-tauri 只能靠 CI 保证"。改完 src-tauri 记得 `touch` 源文件强制重编，避免被缓存结果骗过。
- **SVG 注释里不能出现连续两个连字符**：图标源文件注释里写了 `--accent` 这类 CSS 变量名，
  resvg 直接 panic（`InvalidComment`），且**报错位置指向注释开头**，容易找错地方。
- `npx tauri icon` 会顺带生成 `android/` `ios/` 目录，Windows 项目需手动删除。
- 前端 `xterm` 的默认字体（`courier-new`）和默认 ANSI 调色板（为纯黑背景设计）在深色面板上观感很差，
  这是"字体不清晰、对比度不高"的根因，不是显示器或缩放问题。

## 2026-09-12（Windows 本机同步链路打通）

**本次做了什么**

- 打通 Windows 开发机的 git ↔ GitHub 同步链路（跨机器协作基础设施，非代码改动）：
  - 排查出本机 git 直连 GitHub HTTPS 报 `Empty reply from server`，而本机 `127.0.0.1:7890` 有代理在跑；为 git 配置 `http.proxy` / `https.proxy` 后恢复
  - 配置全局身份 `owlshift <55617812+owlshift@users.noreply.github.com>`（当日随后 GitHub 用户名由 `Dawn-js` 改为 `owlshift`，同步更新了 remote 地址与 README 徽章链接）
  - `credential.helper=manager`，首次 push 完成浏览器授权，凭据已持久化
  - 仓库 clone 至 `C:\Users\sunbo\workbuddy-ai\ssh开发\agent-session-client`

**当前状态**

- Windows 本机可直接 `git pull` / `git push`，无需重复授权。

**下一步计划**

- 建议把仓库默认分支从 `feat/core` 改为 `master`（见下方「踩过的坑」）。
- `~/.ssh/id_ed25519_dawn` 尚未注册到 GitHub 账号；如需 SSH 免交互推送再补。

**踩过的坑**

- **默认分支是 `feat/core`，但它落后 `master` 7 个 merge commit**：新机器 clone 时会报 `remote HEAD refers to nonexistent ref` 且不检出任何文件，必须手动 `git checkout master`。建议在 GitHub 仓库设置里把默认分支改为 `master`。
- 排查网络问题时注意：`curl` 会走 Windows 系统代理而 `git` 不会，两者结果不一致容易误判为「GitHub 挂了」。

## 2026-09-12

**本次做了什么**

- 完成用户反馈的三件事并提交（`5836d6c`）：
  - 应用内配置编辑：后端新增 `save_config` 命令（core 校验 + 写用户配置目录），`ConfigView` 补齐 `user`/`extra_ssh_args`/`cmd` 字段；前端新增设置面板（hosts/agents 增删改）
  - 内置 agent 品牌图标：simple-icons SVG（Claude/DeepSeek/Gemini/OpenAI/Cursor），按 id/label 关键字自动匹配，未知 agent 用首字母+稳定配色字母头像兜底
  - UI 精修：agent 分组卡片、终端标题栏、欢迎页图标、设置入口、滚动条等
- 建立跨机器协作约定：AGENTS.md 增加「常用命令」与「收工规矩」，创建本 WORKLOG。

**当前状态**

- 工作分支 `feat/core`，版本 `0.1.3`；`cargo test`（50）、`npm test`（14）、`npm run build` 全绿。
- clippy 仅一条改动前就存在的警告（`run_session` 参数 8/7），未处理。

**下一步计划**

- 用户在 Windows 上 `npx tauri dev` 目视确认新界面（本机 headless 无法验证 GUI），不满意处再调。
- 确认后提升版本号（如 `0.2.0`）走 CI 发 Release。
- 继续收集真实使用中的摩擦，作为后续 roadmap 来源。

**踩过的坑**

- （历史沉淀见 AGENTS.md「前人踩过的坑」，本次无新增。）
- 注意：本机 headless Linux 无法构建/运行 GUI，界面改动只能靠 tsc/vitest/`cargo test` 保证逻辑，视觉效果必须由有 GUI 环境的机器确认。
- 图标关键字匹配有误伤可能（如 agent id 恰好含 `gpt`），目前可接受，出现再加配置字段。
