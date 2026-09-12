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

/// 注意前面的 `set -g mouse on`：tmux 的 mouse 模式默认是关的，不开的话滚轮
/// 事件在 tmux 里不产生任何滚动（只有 mouse on 时 tmux 才会进 copy-mode 翻历史）。
///
/// 代价：mouse on 之后 tmux 会接管鼠标拖拽选择，想用系统选区复制要按住 Shift。
/// 不想要就在远端执行 `tmux set -g mouse off`。
///
/// 整条命令套在登录 shell 里跑 —— 原因见 `login_shell`。
pub fn build_remote_tmux_cmd(session: &str, agent_cmd: &str) -> String {
    login_shell(&format!(
        "tmux set -g mouse on; tmux set -t {} mouse on 2>/dev/null; tmux new -As {} {}",
        shell_quote(session),
        shell_quote(session),
        shell_quote(agent_cmd)
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
    fn remote_tmux_cmd_runs_in_a_login_shell() {
        // 非交互 ssh 的 PATH 不含 ~/.local/bin（agent 恰好装在那里），而 tmux 新建
        // 会话继承的是**客户端**环境 —— 不套登录 shell，`hermes chat` 会
        // command not found（exit 127），会话随即消失。
        let cmd = build_remote_tmux_cmd("hermes-proj", "hermes chat");
        assert!(cmd.starts_with("${SHELL:-/bin/bash} -lc '"), "{cmd}");
        assert!(cmd.ends_with('\''), "{cmd}");
        assert!(cmd.contains("tmux set -g mouse on"), "{cmd}");
        assert!(cmd.contains("tmux set -t"), "{cmd}");
        assert!(cmd.contains("tmux new -As"), "{cmd}");
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
