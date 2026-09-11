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
        assert_eq!(&argv[1..8], &["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-p", "2222", "-t"]);
        assert_eq!(argv[8], "h");
    }
}
