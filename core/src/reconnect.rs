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

/// Classify an unexpected child exit when ssh stderr is unavailable
/// (it is merged into the PTY stream). Heuristic: a connection that
/// stayed up a while died of a network drop; one that died almost
/// immediately most likely failed to authenticate.
pub fn classify_unexpected_exit(exit_code: Option<i32>, connected_for: Duration) -> SshOutcome {
    const MIN_UPTIME: Duration = Duration::from_secs(60);
    match exit_code {
        Some(255) if connected_for >= MIN_UPTIME => SshOutcome::Network,
        _ => SshOutcome::Unknown,
    }
}

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

    // ---- classify_unexpected_exit ----

    #[test]
    fn long_lived_255_is_network_drop() {
        assert_eq!(
            classify_unexpected_exit(Some(255), Duration::from_secs(61)),
            SshOutcome::Network
        );
    }

    #[test]
    fn short_lived_255_is_not_network() {
        assert_eq!(
            classify_unexpected_exit(Some(255), Duration::from_secs(5)),
            SshOutcome::Unknown
        );
    }

    #[test]
    fn other_exit_codes_are_unknown() {
        assert_eq!(
            classify_unexpected_exit(Some(1), Duration::from_secs(3600)),
            SshOutcome::Unknown
        );
    }

    #[test]
    fn no_exit_code_is_unknown() {
        assert_eq!(
            classify_unexpected_exit(None, Duration::from_secs(3600)),
            SshOutcome::Unknown
        );
    }
}
