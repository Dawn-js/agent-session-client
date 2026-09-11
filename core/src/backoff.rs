use std::time::Duration;

#[derive(Debug)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { base: Duration::from_millis(1000), cap: Duration::from_millis(30000), attempt: 0 }
    }
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let factor = 2u32.saturating_pow(self.attempt.min(5));
        self.attempt = self.attempt.saturating_add(1);
        (self.base * factor).min(self.cap)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn doubles_then_caps_at_30s() {
        let mut b = Backoff::default();
        let got: Vec<u64> = (0..8).map(|_| b.next_delay().as_millis() as u64).collect();
        assert_eq!(got, vec![1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
    }

    #[test]
    fn reset_restarts_sequence() {
        let mut b = Backoff::default();
        let _ = b.next_delay();
        let _ = b.next_delay();
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_millis(1000));
    }
}
