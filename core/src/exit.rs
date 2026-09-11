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
