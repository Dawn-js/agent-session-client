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

/// 用**登录 shell** 跑一条远端命令。
///
/// 非交互 ssh（app 发起的形状）拿到的 PATH 是最小集，**不含 `~/.local/bin`**；
/// 而 agent（hermes / dsh）恰好装在那里，`~/.local/bin` 是由 `~/.profile` 和
/// `~/.bashrc` 加进 PATH 的，这两条非交互 ssh 都不会读。更麻烦的是 tmux 新建
/// 会话继承的是**客户端**环境（不是 tmux server 的全局环境），所以
/// `tmux new -As s 'hermes chat'` 会 `command not found`（exit 127），
/// 会话随即消失（`remain-on-exit` 默认 off）。
///
/// 实测（main，2026-09-12）：
/// ```text
/// 非交互:  PATH=/usr/local/sbin:...  hermes=MISSING
/// 登录shell: PATH=...:/home/ubuntu/.local/bin:...  hermes=/home/ubuntu/.local/bin/hermes
/// ```
///
/// 用 `$SHELL` 而不是写死 `bash`：登录 shell 就是用户真正登录时拿到的那个环境。
/// 会话和探测**必须走同一个形状**，否则又会出现「探测说没装、会话其实能跑」。
pub fn login_shell(cmd: &str) -> String {
    format!("${{SHELL:-/bin/bash}} -lc {}", shell_quote(cmd))
}

/// 滚轮一律由 tmux 自己处理（进 copy-mode 翻历史），不转发给 pane。
///
/// tmux 的**默认**绑定在 pane 申请鼠标时（`#{mouse_any_flag}`）会把滚轮**转发给
/// pane**，而 freebuff / hermes 这类 TUI 申请了鼠标却不响应滚轮 —— 表现就是滚轮
/// 完全没反应。改成不判断 `mouse_any_flag` 即可。
///
/// 实测对照（main，tmux 3.4，pane 里发 `\033[?1000h` 模拟 TUI 申请鼠标）：
/// ```text
/// 默认绑定:   pane_in_mode = 0   （滚轮被转给 pane，tmux 不管）
/// 覆盖绑定后: pane_in_mode = 1   （tmux 接管，进 copy-mode）
/// ```
const WHEEL_UP: &str =
    "bind -n WheelUpPane if -Ft= '#{pane_in_mode}' 'send-keys -M' 'copy-mode -e'";
const WHEEL_DOWN: &str =
    "bind -n WheelDownPane if -Ft= '#{pane_in_mode}' 'send-keys -M' 'send-keys -M'";

/// tmux 里的复制经 OSC 52 送到客户端剪贴板（配合前端的 clipboard addon）。
const CLIPBOARD: &str = "set -g set-clipboard on";

/// 默认 500ms：vim / TUI 里按 ESC 会发粘。
const ESCAPE_TIME: &str = "set -g escape-time 10";

/// `tmux-256color` 的 terminfo 里没有 RGB 能力（`infocmp` 实测为 0），
/// 不补这一条 tmux 只会输出 256 色。双引号是给 tmux 解析用的。
///
/// 用 `set -g` 而不是 `-ga`：`terminal-overrides` 默认是空的（实测），而 `-ga` 是追加 ——
/// 每次连接都会再追加一条，值会无限变长。
const TRUECOLOR: &str = "set -g terminal-overrides \",*256col*:Tc\"";

/// 原生 Windows Terminal 通过 ssh -t 通常给远端的是 xterm-256color；
/// tmux 默认会把 pane 内改成 tmux-256color。Freebuff/OpenTUI 的终端能力判断
/// 在这两种 TERM 下不是同一条路径，导致同样的鼠标点击在原生终端能用、嵌入终端
/// 只能部分命中。让 agent 子进程看到和原生连接一致的 TERM，COLORTERM 补真彩色。
const AGENT_TERM_PREFIX: &str = "env -u TERM_PROGRAM TERM=xterm-256color COLORTERM=truecolor";

pub fn with_native_terminal_env(agent_cmd: &str) -> String {
    format!("{AGENT_TERM_PREFIX} {agent_cmd}")
}

/// 为什么 `mouse` 和滚轮绑定要写成配置文件再 `source-file`，而不是内联几条 `tmux` 命令：
/// 内联时参数要经 login shell **二次解析**，`'send-keys -M'` 里的空格会被拆成两个参数，
/// tmux 直接报 `if-shell: too many arguments`（实测 exit=1）。写进配置文件则由 tmux
/// 自己解析引号，一次就位。
///
/// `set -t <session> mouse on` 是给**已有**会话补的：会话可能自己存了 `mouse off`，
/// 会话级选项会盖过全局的。
///
/// 代价：mouse on 之后 tmux 会接管鼠标拖拽选择，想用系统选区复制要按住 Shift；
/// pane 里真正想收滚轮的 TUI 会收不到（但它们本来也不响应）。
///
/// 整条命令套在登录 shell 里跑 —— 原因见 `login_shell`。
pub fn build_remote_tmux_cmd(session: &str, agent_cmd: &str) -> String {
    login_shell(&format!(
        "printf '%s\\n' {mouse} {clip} {esc} {tc} {up} {down} > /tmp/asc-tmux.conf; \
         tmux source-file /tmp/asc-tmux.conf; \
         tmux set -t {s} mouse on 2>/dev/null; \
         tmux new -As {s} {cmd}",
        mouse = shell_quote("set -g mouse on"),
        clip = shell_quote(CLIPBOARD),
        esc = shell_quote(ESCAPE_TIME),
        tc = shell_quote(TRUECOLOR),
        up = shell_quote(WHEEL_UP),
        down = shell_quote(WHEEL_DOWN),
        s = shell_quote(session),
        cmd = shell_quote(&with_native_terminal_env(agent_cmd)),
    ))
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

/// probe 远端会话是否已存在。和 `build_exec_argv` 一样加 `ConnectTimeout` / `BatchMode`：
/// 主机不可达时不能让探测把那 2 分钟 TCP 超时当成「关闭信号的盲区」——
/// 会话关闭要等探测返回才看得见（见 `reconnect::wait_for_close`）。
/// `Command::output()` 的 stdin 是 null，需要口令的认证本来就过不去，所以 BatchMode 不改变结果。
pub fn build_probe_argv(target: &SshTarget, session: &str) -> Vec<String> {
    let mut argv = base_argv(target);
    argv.push("-o".into());
    argv.push("ConnectTimeout=10".into());
    argv.push("-o".into());
    argv.push("BatchMode=yes".into());
    argv.push(destination(target));
    argv.push(format!("tmux has-session -t {}", shell_quote(session)));
    argv
}

/// 一次性远端命令（非交互）。用于「问一句就回」的场景（目录列表），
/// 走独立 ssh 进程，不占 PTY 会话、不污染终端输出。
///
/// 比 `build_probe_argv` 多两个 `-o`，都是为了让 UI 调用有确定的失败时机：
/// `ConnectTimeout=10` 防止主机不可达时面板一直转圈；
/// `BatchMode=yes` 让需要口令的认证直接失败，而不是挂在提示符上。
pub fn build_exec_argv(target: &SshTarget, cmd: &str) -> Vec<String> {
    let mut argv = base_argv(target);
    argv.push("-o".into());
    argv.push("ConnectTimeout=10".into());
    argv.push("-o".into());
    argv.push("BatchMode=yes".into());
    argv.push(destination(target));
    argv.push(cmd.to_string());
    argv
}

/// 判定一条**一次性** ssh exec 的结果。
///
/// 只要有 stdout 就用它：探测命令是 shell 循环，可能部分失败（末尾的 bin 不存在
/// 会让整条命令非零退出），但既然拿到了输出就说明连上了，筛选交给解析层。
/// 只有「没有输出且退出码非零」才算真失败，报 stderr 原文 —— ssh 的报错都在那里。
///
/// 注意 `code == 0 && stdout.is_empty()` 是**合法答案**（一台 agent 都没装），
/// 不是失败；把它当错误会让「该主机上没有已知 agent」这条引导文案永远看不到。
pub fn probe_outcome(code: i32, stdout: &str, stderr: &str) -> Result<String, String> {
    if code == 0 || !stdout.trim().is_empty() {
        return Ok(stdout.to_string());
    }
    let msg = stderr.trim();
    Err(if msg.is_empty() {
        format!("远端命令退出码 {code}")
    } else {
        msg.to_string()
    })
}

/// 滚轮滚动：先确保进 copy-mode，再在其中滚 N 行。
///
/// **为什么不走客户端按键**（`Ctrl+b [`）：那条链路要经过 xterm → IPC → PTY →
/// ssh → tmux 客户端，而实测用户环境里这些按键**到不了 tmux**（tmux 侧本身正常：
/// 用 pty 客户端发 `\x02[` 时 `pane_in_mode=1`）。走这条独立的命令通道就绕开了
/// 整条链路，只依赖"能执行远端命令"这一件事（文件面板已经在用）。
///
/// 已在真实 tmux 上验证：`copy-mode` 后 `scroll-up` 让 `#{scroll_position}`
/// 从空 → 3 → 23。
///
/// pane 在 alternate screen 时（freebuff 这类全屏 TUI）没有历史可滚，
/// 此时命令仍会成功但看不出变化 —— 那是 TUI 自身的限制。
///
/// **`-e` 不能省**（踩过）：它是"滚到底部时退出 copy-mode"。少了它，用户滚完之后
/// 会**一直留在 copy-mode**，而 copy-mode 会把键盘输入全部吃掉 —— 表现成
/// "这个会话突然无法输入"。实测：hermes `pane_in_mode=1`、`scroll_position=0`
/// （已经在底部却仍不退出），同期没滚过的 codebuddy 是 0。
pub fn build_scroll_cmd(session: &str, up: bool, lines: u32) -> String {
    format!(
        "tmux copy-mode -e -t {s} 2>/dev/null; tmux send-keys -t {s} -X -N {n} {dir}",
        s = shell_quote(session),
        n = lines,
        dir = if up { "scroll-up" } else { "scroll-down" },
    )
}

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
    fn agent_gets_native_terminal_environment() {
        assert_eq!(
            with_native_terminal_env("freebuff --project"),
            "env -u TERM_PROGRAM TERM=xterm-256color COLORTERM=truecolor freebuff --project"
        );
    }

    #[test]
    fn remote_tmux_command_sets_native_terminal_environment() {
        let cmd = build_remote_tmux_cmd("freebuff", "freebuff");
        assert!(
            cmd.contains("env -u TERM_PROGRAM TERM=xterm-256color COLORTERM=truecolor freebuff"),
            "{cmd}"
        );
    }

    #[test]
    fn remote_tmux_cmd_runs_in_a_login_shell() {
        // 非交互 ssh 的 PATH 不含 ~/.local/bin（agent 恰好装在那里），而 tmux 新建
        // 会话继承的是**客户端**环境 —— 不套登录 shell，`hermes chat` 会
        // command not found（exit 127），会话随即消失。
        let cmd = build_remote_tmux_cmd("hermes-proj", "hermes chat");
        assert!(cmd.starts_with("${SHELL:-/bin/bash} -lc '"), "{cmd}");
        assert!(cmd.ends_with('\''), "{cmd}");
        // mouse 与滚轮绑定现在写在配置文件里再 source（见 WHEEL_UP 的说明）
        assert!(cmd.contains("set -g mouse on"), "{cmd}");
        assert!(cmd.contains("tmux source-file"), "{cmd}");
        assert!(cmd.contains("tmux set -t"), "{cmd}");
        assert!(cmd.contains("tmux new -As"), "{cmd}");
    }

    #[test]
    fn wheel_is_routed_to_tmux_not_forwarded_to_the_pane() {
        // tmux 的默认 WheelUpPane 绑定在 pane 申请鼠标时（#{mouse_any_flag}）会把
        // 滚轮**转发给 pane**。freebuff / hermes 这类 TUI 申请了鼠标却不响应滚轮，
        // 于是滚轮完全没反应 —— 必须覆盖成「无论 pane 是否申请，都由 tmux 处理」。
        let cmd = build_remote_tmux_cmd("hermes-proj", "hermes chat");
        assert!(cmd.contains("bind -n WheelUpPane"), "{cmd}");
        assert!(cmd.contains("bind -n WheelDownPane"), "{cmd}");
        assert!(cmd.contains("copy-mode -e"), "{cmd}");
        assert!(
            !cmd.contains("mouse_any_flag"),
            "不能保留「pane 申请了就把滚轮转发给它」的判定：{cmd}"
        );
    }

    #[test]
    fn builds_scroll_cmd_entering_copy_mode_first() {
        let up = build_scroll_cmd("hermes", true, 3);
        // 必须先 copy-mode，否则 send-keys -X 没有作用的模式
        assert!(up.contains("tmux copy-mode -e -t 'hermes'"), "{up}");
        assert!(up.contains("send-keys -t 'hermes' -X -N 3 scroll-up"), "{up}");
        assert!(build_scroll_cmd("hermes", false, 3).contains("scroll-down"));
        // -e（滚到底自动退出）不能丢：少了它用户会永久卡在 copy-mode 里无法输入
        assert!(up.contains("copy-mode -e"), "{up}");
    }

    #[test]
    fn scroll_cmd_quotes_hostile_session_names() {
        // 会话名来自配置，可能带空格或引号 —— 不能直接拼进命令
        let cmd = build_scroll_cmd("a b'c", true, 5);
        assert!(cmd.contains("'a b'\\''c'"), "{cmd}");
    }

    #[test]
    fn tmux_config_carries_clipboard_keys_and_truecolor() {
        // 这三条要跟着应用走，不能只活在某一台机器的 ~/.tmux.conf 里：
        // - set-clipboard on：tmux 里复制经 OSC 52 进 Windows 剪贴板
        // - escape-time 10：默认 500ms 会让 vim/TUI 里的 ESC 发粘
        // - terminal-overrides Tc：tmux-256color 的 terminfo 里没有 RGB，不补就降级成 256 色
        let cmd = build_remote_tmux_cmd("hermes-proj", "hermes chat");
        assert!(cmd.contains("set-clipboard on"), "{cmd}");
        assert!(cmd.contains("escape-time 10"), "{cmd}");
        assert!(cmd.contains("Tc"), "真彩色 override 不能少：{cmd}");
    }

    #[test]
    fn wheel_binding_goes_through_a_config_file_not_inline() {
        // 内联的 `tmux bind ... if -Ft= '...' '...'` 活不过 login shell 的二次解析：
        // 参数里的空格会被拆开，tmux 报 `if-shell: too many arguments`（实测 exit=1）。
        // 只能写进配置文件让 tmux 自己解析引号 —— 所以这里要求走 source-file。
        let cmd = build_remote_tmux_cmd("hermes-proj", "hermes chat");
        assert!(cmd.contains("source-file"), "绑定必须经配置文件下发：{cmd}");
        assert!(
            cmd.contains("'#{pane_in_mode}'"),
            "tmux 的引号必须原样落到配置内容里：{cmd}"
        );
    }

    #[test]
    fn login_shell_quotes_the_inner_command() {
        assert_eq!(login_shell("echo hi"), "${SHELL:-/bin/bash} -lc 'echo hi'");
        // 内层单引号必须转义，否则命令到那里就被截断了
        assert_eq!(login_shell("echo 'x'"), "${SHELL:-/bin/bash} -lc 'echo '\\''x'\\'''");
    }

    #[test]
    fn builds_session_argv_with_keepalive_and_tty() {
        let argv = build_session_argv(&target(), "hermes-proj", "hermes chat");
        let head = vec![
            "ssh",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-t", "ubuntu@example.com",
        ];
        assert_eq!(&argv[..head.len()], &head[..]);
        assert_eq!(argv.len(), head.len() + 1);
        assert_eq!(argv[head.len()], build_remote_tmux_cmd("hermes-proj", "hermes chat"));
    }

    #[test]
    fn builds_probe_argv_without_tty() {
        assert_eq!(build_probe_argv(&target(), "hermes-proj"), vec![
            "ssh",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-o", "ConnectTimeout=10", "-o", "BatchMode=yes",
            "ubuntu@example.com",
            "tmux has-session -t 'hermes-proj'",
        ]);
    }

    #[test]
    fn extra_ssh_args_precede_destination() {
        let t = SshTarget { host: "h".into(), user: None, extra_ssh_args: vec!["-p".into(), "2222".into()] };
        let argv = build_session_argv(&t, "s", "c");
        assert_eq!(&argv[1..8], &["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-p", "2222", "-t"]);
        assert_eq!(argv[8], "h");
    }

    #[test]
    fn builds_exec_argv_without_tty_and_with_bounded_wait() {
        assert_eq!(build_exec_argv(&target(), "ls -1"), vec![
            "ssh",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-o", "ConnectTimeout=10", "-o", "BatchMode=yes",
            "ubuntu@example.com",
            "ls -1",
        ]);
    }

    #[test]
    fn probe_outcome_returns_stdout_on_success() {
        assert_eq!(probe_outcome(0, "hermes\n", "").unwrap(), "hermes\n");
    }

    #[test]
    fn probe_outcome_keeps_stdout_even_on_nonzero_exit() {
        // 恰好最后一个 bin 没装时远端可能给非零退出码，但输出是真的，不能丢
        assert_eq!(probe_outcome(1, "hermes\n", "noise").unwrap(), "hermes\n");
    }

    #[test]
    fn probe_outcome_reports_stderr_when_there_is_no_output() {
        let err = probe_outcome(255, "", "ssh: Could not resolve hostname nope").unwrap_err();
        assert_eq!(err, "ssh: Could not resolve hostname nope");
    }

    #[test]
    fn probe_outcome_falls_back_to_exit_code_when_stderr_is_empty() {
        assert_eq!(probe_outcome(255, "", "").unwrap_err(), "远端命令退出码 255");
    }

    #[test]
    fn probe_outcome_treats_empty_success_as_a_real_answer() {
        // 「一台都没装」是合法结果，不是失败
        assert_eq!(probe_outcome(0, "", "").unwrap(), "");
    }
}
