# WORKLOG

> 倒序排列，最新在顶部。每次会话收工前更新（见 AGENTS.md「收工规矩」）。

## 2026-09-13（终端体验：搜索 / 链接 / 剪贴板 / 真彩色 / resize 防抖）

**本次做了什么**

- **搜索**：`@xterm/addon-search`。`Ctrl+Shift+F` 打开浮层（`attachCustomKeyEventHandler`
  拦下来，不让它变成远端输入），Enter 下一个 / Shift+Enter 上一个 / Esc 关闭。
- **链接**：`@xterm/addon-web-links`。点不开时（Tauri 的 webview 会拦 `window.open`）
  退化成复制链接 —— 这样不用引 opener 插件。
- **剪贴板**：`@xterm/addon-clipboard` + tmux `set-clipboard on`，tmux 里的复制经 OSC 52
  进 Windows 剪贴板。
- **tmux 配置**（仍走 `printf` + `source-file`，见 ledger 18）新增三条：
  `set-clipboard on`、`escape-time 10`（默认 500ms 会让 vim/TUI 的 ESC 发粘）、
  `set -g terminal-overrides ",*256col*:Tc"`（`tmux-256color` 的 terminfo 实测没有 RGB，
  不补只出 256 色）。
- **resize 防抖**：前端 120ms debounce + 后端尺寸去重 —— 之前 ResizeObserver 每次抖动都会
  `fit()` 并发 resize，后端每次都注入一遍 `refresh-client` 序列。

**刻意跳过的（附理由）**

- `extended-keys on`：**xterm.js 不支持** `modifyOtherKeys`/kitty 键盘协议
  （源码里 grep 不到），设了也是空转。
- `default-terminal tmux-256color`：tmux 3.4 的默认值就是它，多余。
- 解析 Windows Terminal schemes：只有两套主题需求，为兼容别人的格式写解析器不划算。
- Ligatures / Mica 无边框 / WebGL addon / 二进制 channel：收益不明或成本与收益不匹配。
- **Ctrl+C 继续拦截**（用户明确选择保持现状）：清单建议"Ctrl+C 发 SIGINT、复制用
  Ctrl+Shift+C"，但那会放弃「误触不丢会话」的保护（见 `keys::strip_quit_keys`）。

**当前状态**

- `cargo test` 98 passed（core）+ shim 4；`cargo check` 0 warning；`npm test` 33；
  `npm run build` 通过（bundle 504KB，三个 addon 带来的，暂不做 code-split）。
- 实测：新配置 `source-file` exit=0，`set-clipboard on` / `escape-time 10` /
  `terminal-overrides[0] *256col*:Tc` 均生效，7 个会话未受影响。

## 2026-09-13（滚轮真因、待办面板、拖拽与关闭重连）

**本次做了什么**

- **滚轮无效的真因**：不是 `mouse` 没开（应用早就设了），而是 tmux 的**默认**
  `WheelUpPane` 绑定在 pane 申请鼠标时（`#{mouse_any_flag}`）把滚轮**转发给 pane**。
  freebuff / hermes 这类 TUI 申请了鼠标却不响应滚轮 —— 于是滚轮像坏了一样。
  改成不判断 `mouse_any_flag`、一律由 tmux 进 copy-mode。
  实测对照（pane 里发 `\033[?1000h` 模拟 TUI）：默认 `pane_in_mode=0`，覆盖后 `=1`。
- **绑定必须走配置文件**：内联 `tmux bind ... if -Ft= '...' '...'` 活不过 login shell 的
  二次解析 —— 参数里的空格被拆开，tmux 报 `if-shell: too many arguments`（实测 exit=1）。
  改成 `printf` 写 `/tmp/asc-tmux.conf` 再 `tmux source-file`，由 tmux 自己解析引号。
- **右侧「技能」页签改为「待办」**：localStorage 持久化、全局共用（不跟会话走）。
  顺带删掉不再使用的 `list_skills` 命令与 core 的 skills 解析及其测试。
- **修拖拽**：`tauri.conf.json` 补 `dragDropEnabled: false` —— Windows 上 Tauri 默认
  拦截 HTML5 拖放，文件面板的 `onDrop` 从来没被触发过。
- **修「关掉后连不上」**：`close_session` 发完 `__close` 就返回，而 runner 还要 kill PTY、
  wait 子进程、清会话表；这期间重开同一会话会撞上 `start_session` 的重复保护。
  现在等到会话表清掉（最多 5s）再返回。
- **修「打字每个字重复」**：xterm 的 `dispose()` **不摘掉自己插入的 DOM**，而 `Terminal`
  的 effect 依赖 `onData`（依赖 `active`），切会话就会重建 —— 新旧两棵树叠着渲染。
  两处一起改：cleanup 里 `replaceChildren()` 清容器，`onData`/`onResize` 改用 ref 读
  `active` 保持稳定。
- **补一手**：回调稳定之后，切会话不再重建终端，上一个会话的画面就会留在屏上、新会话的
  输出直接叠上去。给 `Terminal` 加 `key={active}`，把「一会话一终端实例」表达清楚
  （重建本身是安全的，因为 cleanup 已经会清容器）。

**当前状态**

- `cargo test`：97 passed（core）+ shim 4 passed；`cargo check` 0 warning；`npm test` 33 passed。
- 用户的 `~/.tmux.conf` **由本次会话创建**（原来不存在）：`set -g mouse on`、
  `set -g history-limit 50000`、两条 Wheel 绑定。已 `source-file`，7 个会话未受影响。

**踩过的坑**

- **两层 shell 解析会吃掉 tmux 的引号**：`shell_quote` 把整段包进单引号后，bash 的第二次
  解析让 `'send-keys -M'` 变成未分组的两个参数。凡是给 tmux 传「带空格的参数」，
  都得走配置文件，别内联。
- 前面那版 `TMUX_REFRESH`（`\x02:refresh-client\r`）注入与此无关：它在 tmux 前缀键之后，
  由 tmux 客户端消费，不会进 pane。

## 2026-09-13（修 freebuff「界面显示不全」：resize 后强制 tmux 全量重绘）

**本次做了什么**

- 用户报告：freebuff 界面显示不全、滚轮无效。在 Xvfb 真机复现并逐层定位（字节流重放
  实验：tmux 发出的流重放进 headless xterm 与 tmux 自身视图逐字一致 → 问题在客户端
  resize 竞态，不在传输/解析）。
- **根因**：PTY 按 spawn 时硬编码的 80x24 收第一帧，前端 FitAddon 随即发 `__resize`；
  tmux 对 resize 只补差量，新旧内容重叠的格子永远不被重写，错字/错位滚动条永久残留。
- **修法**：runner 处理 `__resize` 后经 PTY 发 tmux 前缀键 `C-b :refresh-client`，
  强制全量重绘。键被外层 tmux 客户端消费，不会进 agent。Xvfb 目视验证：重连后画面与
  `tmux capture-pane` 完全一致，无残留。
- **滚轮无效是 freebuff 自身缺陷，客户端无解**：它启动时申请鼠标上报（`mouse_any=1`）
  但对 SGR 滚轮事件（press+release 成对、上下都试了）完全不响应；且跑在 alternate
  screen，xterm 侧也无回滚。真终端直连同样无效。
- 顺带发现：freebuff 单例锁被一个孤儿实例（pts/12，不在任何 tmux 会话）占着，用户会话
  一点 Open 就弹 "already running"。已在用户会话里 Take over 回锁。

**当前状态**

- `cargo test`：100 passed（core）+ shim 4 passed；`cargo check` 0 warning；
  `npm test` 28 passed；`npm run build` 通过。
- A 机器 dev 配置（`~/.config/dev.local.agent-session-client/config.json`）加了 freebuff
  agent 便于测试，保留。
- 新坑备录：从无 TERM 的环境启动 app 时 ssh 带 `TERM=dumb`，tmux 客户端直接拒 attach
  （`terminal does not support clear`）。Windows 不受影响；dev 跑 GUI 要 `TERM=xterm-256color`。

**下一步计划**

- 考虑在 transport 里兜底设 `TERM=xterm-256color`（小改，防 dumb 环境）。
- freebuff 的滚轮/历史滚动只能在它自家修；可在 README 注明。

## 2026-09-12（首启流程真机验证 + 修「会话找不到 agent」）

**本次做了什么**

- **在本机把 GUI 真跑起来做了目视验证** —— 推翻了「本机没有 webkit2gtk」的旧判断
  （`webkit2gtk-4.1` 是装着的）。方法已写进 AGENTS.md「开发环境 A」：Xvfb + ffmpeg x11grab
  截图 + ctypes 调 libXtst 点击，零安装。从此**不用每次都等 Windows 目视确认**。
- **首启流程全链路验证通过**（截图逐帧确认）：无配置 → 「选择一台服务器开始」列出
  `~/.ssh/config` 里的别名 → 点别名 → 探测 → 列出探到的 agent → 确认 → 配置落盘 →
  进主界面且只有探到的 agent。落盘位置是 `~/.config/dev.local.agent-session-client/config.json`
  （dev 构建的 identifier 带 `dev.local.` 前缀，别去 `~/.config/agent-session-client` 找）。
  内容核对过：只有点过的 host、只有探到的两个 agent。错误路径也验了（探测 github.com，
  GitHub sshd 的拒绝信息原样显示）。
- 顺手修 `.row` 不换行导致别名按钮溢出侧栏（`1334e64`）。
- **修根因「会话里找不到 agent」**：非交互 ssh 的 PATH 不含 `~/.local/bin`（agent 就装在那），
  而 tmux 新建会话继承的是**客户端**环境，不是 server 全局环境 —— 所以
  `tmux new -As s 'hermes chat'` 会 `command not found`（EXIT=127），会话瞬间消失。
  新增 `command::login_shell`，**会话和探测两处一起**套登录 shell（只改一边就会
  「探测说没装、会话其实能跑」）。实测：套上之后 `hermes` 能解析到
  `/home/ubuntu/.local/bin/hermes`，`dsh` 也能执行了。
- 之前 `hermes` 按钮"能用"是假象：tmux 上已有一个手动从登录 shell 建的 `hermes` 会话，
  `tmux new -As` 在会话已存在时只是 attach、命令根本不执行。

**当前状态**

- `cargo test`：99 passed（core）+ transport shim 4 passed。
- `cargo check -p agent-session-client`：通过，0 warning。
- `npm test`：28 passed。`npm run build`：通过。
- master HEAD 含未推送提交（见 git log）。

**下一步计划**

- **`dsh` 的注册表启动命令不完整**：登录 shell 修好后 `dsh` 能执行了，但报
  `error: --profile <name> is required`。`agents.rs` 里写的 `"dsh"` 需要真实参数
  （或让用户在配置里自己填）。`hermes chat` 的端到端没测（不想为验证多起一个真 agent），
  PATH 解析已单独验证过。
- 文件面板的长驻通道（`filechan`）**仍是**非交互 PATH。目前它只跑 `ls`/`find`/`cat`
  （都在 `/usr/bin`）所以没事，但以后若要在文件面板命令里用到 `~/.local/bin` 的东西，
  会踩同一个坑 —— AGENTS.md ledger 第 17 条已记。
- 版本还在 0.2.3；下次发版前记得四处一起 bump（这次改动含用户可见行为，值得发一版）。

**踩过的坑**

- `pkill -f "Xvfb :99"` 会匹配到自己命令行把父 shell 一起杀掉（表现为后台任务秒挂、
  输出全空）。要用 `pkill -x Xvfb`。已记入 AGENTS.md 开发环境一节。
- **在交互终端里测远端命令是骗人的**：`command -v hermes` 在交互 shell 里找得到、
  在 app 的非交互 ssh 里找不到。判断 app 里能不能跑，用
  `ssh -o BatchMode=yes <host> '<命令>'` 复现。已记入 AGENTS.md ledger 第 17 条。
- 验证 UI 交互不需要 xdotool：`libXtst.so.6` + python ctypes 十行就够（见 AGENTS.md）。

## 2026-09-12（首启探测 + 探测与文件面板解耦）

**本次做了什么**

- **修「首次启动没有任何引导，进去就是自动配置，也没探测我的 host」**。根因不是探测功能本身，
  是探测在首启**结构上就调不到**，三处叠在一起：
  1. `App.tsx` 的 `useAlias()` 走的是 `knownAgents()`（后端注册表**全集**），从头到尾没调
     `probeAgents` —— 于是把服务器上根本没装的 agent 写进配置，点它必挂。
  2. `probe_agents` → `exec_remote` → `cfg.to_ssh_target(host)`，只认**已保存配置里的 host 名**
     （`SettingsModal.tsx` 的注释原文写明了这条约束）。首启没有配置 → 没有 host 可解析 →
     探测不可能调到，只能退化成猜。
  3. 探测还借了文件面板那条长驻 ssh（`state.file_chan`）：`once()` 全程持锁跑 `FileChan::run`
     （`CMD_TIMEOUT = 30s`，失败还会 `take()` 掉重建再跑一次），既会和文件面板互相等锁，
     host 不同还会把文件面板的通道直接顶掉。
- **解耦（core）**：新增 `config::resolve_target(config, host_or_alias)` —— 先在配置里按 `name`
  查，查不到就把这个字符串当成 ssh 别名原样交给系统 ssh（`~/.ssh/config` 去解释
  HostName/User/Port）。探测因此不再依赖已保存的配置，首启可用。
- **解耦（传输）**：新增 `commands::exec_remote_oneshot`（`spawn_blocking` + `Command::output()` +
  `hide_console`，复用已有的 `build_exec_argv`），`probe_agents` 改走它，不再碰 `state.file_chan`。
  输出判定抽成 `command::probe_outcome`：有 stdout 就用（避免「最后一个 bin 没装 → 非零退出」
  把真结果丢掉），`code == 0 && 输出为空` 是**合法答案**「一台都没装」而不是失败。
- **首启引导（前端）**：`bootstrapConfigJson` 在一个都没探到时返回 `null`，绝不写一份必然被后端
  校验拒掉的配置；`ConfigErrorPanel` 自己持有探测状态 —— 点别名 → 「正在探测 X…」→ 列出探到的
  agent → 确认后**只把探到的**写进配置；一个都没探到就说明原因并引导改用手填模板。
  删掉 `useAlias`（那条猜的路）。

**当前状态**

- `cargo test`：98 passed（core）+ transport shim 4 passed。
- `cargo check -p agent-session-client`：通过，0 warning。
- `npm test`：28 passed（`configEdit` 新增「空探测结果返回 null」，替换掉原来那条以
  「两边列表都非空」为由的测试 —— 那条测试的理由正是本 bug 的成因）。
- `npm run build`：通过（tsc 严格 + vite）。
- **未经 Windows 目视确认**：探测中的按钮禁用态、探到的 agent 列表、确认保存后的界面跳转。

**下一步计划**

- 在 Windows 上 `npx tauri dev` 实测首启：把配置移走 → 应出现「选择一台服务器开始」→ 点别名 →
  看到「正在探测」→ 看到探到的 agent 列表 → 确认后进入主界面，且**只有**探到的 agent。
- **发版前必须先 bump 版本**（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` /
  `Cargo.lock` 四处一起改），否则 push 到 master 会把新产物覆盖到 `v0.2.2` 那个 tag 上 ——
  v0.2.1 就是这么被污染的。

**踩过的坑**

- **「首启要用的能力」不能依赖「首启时还不存在的东西」**：探测只认已保存配置里的 host 名，
  而首启恰恰没有配置文件，于是探测结构上就调不到，只能退化成猜。已沉淀为 AGENTS.md ledger 第 16 条。
- **不要把一次性操作搭在长驻通道上**：借用方会和通道主人互相等锁，换 host 还会把对方的通道顶掉
  （`once()` 里 `*guard = Some(FileChan::spawn(...))`，旧的 Drop 会 kill 掉 ssh）。要借用之前
  先想清楚谁阻塞谁。

## 2026-09-12（关闭会话 + 探测已装 agent）

**本次做了什么**

- **修「连接错误时会话关不掉」**。用户报的现象背后其实是三个叠着的 bug：
  1. 前端根本没有关闭入口 —— `close_session` 命令一直在，但从未被调用。
  2. **根因**：runner 只在 PTY 输入循环里读输入通道，而网络类失败会无限重连
     （`is_retryable` 只认 Network），退避期间是 `thread::sleep`，`__close` 永远积压。
     于是连接错误的会话既关不掉、又一直在后台重连。
     修法：core 新增 `reconnect::wait_for_close`（≤100ms 切片轮询，能被 `__close` 打断，
     且不会丢掉退避期间到达的 `__resize`）与 `drain_pending`（用在阻塞的 `probe_session`
     返回之后）；runner 的两处退避 sleep 与探测后各接上关闭路径，关闭收尾抽成 `finish_close`。
  3. 前端 `applyState` 对未知 id 是追加，本地删掉的行会被后续 `exited`/`retrying` 事件复活。
     修法：App 里用 `useRef<Set>` 立「已关闭」墓碑，墓碑 id 的 state/output/notice 一律丢弃；
     `start()` 重新开始同一 id 时撤掉墓碑。会话行拆成 `li > .session-main + .session-close`（button 不能嵌套）。
- `build_probe_argv` 补 `ConnectTimeout=10` / `BatchMode=yes`：主机不可达时探测要阻塞约 2 分钟，
  而关闭信号要等探测返回才看得见。`Command::output()` 的 stdin 是 null，需要口令的认证本来就过不去，
  所以 BatchMode 不改变结果。
- **新增「探测服务器已装 agent」**：`core/src/agents.rs` 放已知 agent 注册表
  （hermes / dsh，加一行即可扩充）+ `command -v` 探测命令；`probe_agents` 命令走文件面板那条长驻 ssh；
  设置面板里选 host → 探测 → 「加入 / 全部加入」写进可编辑列表，点保存才落盘（不自动改配置）。
  探测刻意不改 PATH：用的就是会话启动时同一个非登录 shell 环境，能探到 = 真能启动。

**当前状态**

- `cargo test -p session_core`：77 passed + transport shim 4 passed。
- `cargo check -p agent-session-client`：通过，0 warning。
- `npm test`：24 passed（新增 removeSession / mergeDiscoveredAgents）。
- `npm run build`：通过（tsc 严格 + vite）。
- **未经 Windows 目视确认**：关闭中/重连中/连接失败后关闭、探测结果加入后保存的界面效果。

**下一步计划**

- 在 Windows 上 `npx tauri dev` 实测：点开一个连不上的 host，确认徽标在「重连中」，点 ✕ 能立刻消失且不再复活；
  再在设置面板里对 main 探测，确认能列出 hermes/dsh 并加入保存。
- 关闭按钮目前只断开本地连接、保留远端 tmux（与设计 §7.5 一致）；「同时结束远端会话」暂无 UI。

**踩过的坑**

- **退避 sleep 是控制帧的盲区**：任何「等待期间也要能收到用户意图」的循环，都不能用裸 `thread::sleep`。
  已沉淀为 AGENTS.md 踩坑 ledger 第 15 条。
- **`for ... done` 的退出码取最后一次执行**：探测命令里最后一个 bin 没装就是 1，
  而长驻通道把非零退出码当失败，所以命令结尾必须补 `; true`。

## 2026-09-12（滚轮二次修复）

**本次做了什么**

- 用户反馈上一版滚轮仍无效。根因确认：只执行 `tmux set -g mouse on` 不够；已有 tmux 会话可能在 session 级别保留 `mouse off`，覆盖全局选项。
- `build_remote_tmux_cmd` 现在先执行 `tmux set -g mouse on`，再执行 `tmux set -t '<session>' mouse on 2>/dev/null`，最后 `tmux new -As ...`。
  这样已有会话和新建会话都能开启 mouse；新建时 session 不存在，定向设置失败会被忽略，不影响后续创建。
- 在 main 上做了真实 tmux 验证：先把目标会话设为 `mouse off`，再执行新命令，`tmux show-options -t <session> -v mouse` 返回 `on`。

**验证**

- `cargo test -p session_core`：62 passed + transport shim 4 passed。
- `cargo check -p agent-session-client`：通过。
- `npm run build`：通过。
- `npm test`：19 passed。

**下一步**

- 推送并生成新版 Windows 安装包，用户验证已有会话滚轮。

## 2026-09-12（终端选中复制）

**本次做了什么**

- `Terminal.tsx` 监听 xterm 的 `onKey`：有选区时 Ctrl+C / Cmd+C 复制当前选中文本到系统剪贴板，阻止 ETX 继续发给远端；无选区时保持原有安全策略，不改变远端输入行为。
- 右键交互改为：有选区就复制，没有选区就粘贴。仍使用 WebView2 用户手势允许的 `navigator.clipboard`，不引入新依赖。
- 新增 `src/terminalClipboard.ts` 和 5 个测试，覆盖 Ctrl+C 有/无选区、Cmd+C、普通 c、右键复制/粘贴。

**当前状态**

- `npm run build` 通过；`npm test` 通过：4 个测试文件、19 项测试全绿。
- Windows 端尚未目视确认；需要实际选中文字后按 Ctrl+C，再到记事本验证剪贴板内容。

**下一步计划**

- 同步到 `main` 做 Rust/Tauri crate 检查，推送并生成新的 Windows 安装包。
- 重点体验：拖动选区、Ctrl+C 复制、无选区右键粘贴、有选区右键复制。

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
