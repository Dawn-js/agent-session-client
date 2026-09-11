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
