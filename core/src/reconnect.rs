use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use crate::backoff::Backoff;
use crate::exit::{is_retryable, SshOutcome};

/// 关闭会话的控制帧。由 `close_session` 经会话输入通道发出。
/// 定义在 core 里供 runner 与 src-tauri 共用，避免协议字符串各处漂移。
pub const CLOSE_FRAME: &[u8] = b"__close";

const RESIZE_PREFIX: &[u8] = b"__resize:";

/// 解析 `__resize:<cols>x<rows>` 控制帧。非法帧返回 None（调用方丢弃即可）。
pub fn parse_resize_frame(frame: &[u8]) -> Option<(u16, u16)> {
    let dims = frame.strip_prefix(RESIZE_PREFIX)?;
    let s = std::str::from_utf8(dims).ok()?;
    let (cols, rows) = s.split_once('x')?;
    Some((cols.parse().ok()?, rows.parse().ok()?))
}

/// 在退避等待期间也能收到用户的关闭请求。
///
/// 不能直接用 `thread::sleep`：网络类失败会无限重连（`is_retryable` 只认 Network），
/// 而 sleep 期间没人读输入通道，`__close` 就一直积压 —— 会话于是永远关不掉。
///
/// 以 ≤100ms 切片轮询，返回 true 表示**调用方必须立即走关闭路径**：
/// `__close` 帧已经被这里消费掉，再进下一轮就永久丢失了。
///
/// 等待期间没有 PTY，普通按键没有可写入的对象，直接丢弃（断线输入本就不该回放）。
/// 但 `__resize` 必须留下并写回 `dims`，否则重连出来的 PTY 会用旧尺寸，
/// 而前端只在尺寸真正变化时才发 resize，不会补发。
pub fn wait_for_close(rx: &Receiver<Vec<u8>>, delay: Duration, dims: &mut (u16, u16)) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(frame) => {
                if frame.as_slice() == CLOSE_FRAME {
                    return true;
                }
                if let Some(size) = parse_resize_frame(&frame) {
                    *dims = size;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // 发送端没了（会话表已清），不可能再收到关闭请求 —— 等同于关闭
            Err(RecvTimeoutError::Disconnected) => return true,
        }
    }
}

/// 非阻塞地把通道里已积压的控制帧处理掉，返回是否应当关闭。
///
/// 用在阻塞的 `probe_session` 返回之后：那次 ssh 最长要等 ConnectTimeout(10s)，
/// 期间用户完全可能已经点了关闭，不处理掉就会又 spawn 一个 PTY。
pub fn drain_pending(rx: &Receiver<Vec<u8>>, dims: &mut (u16, u16)) -> bool {
    loop {
        match rx.try_recv() {
            Ok(frame) => {
                if frame.as_slice() == CLOSE_FRAME {
                    return true;
                }
                if let Some(size) = parse_resize_frame(&frame) {
                    *dims = size;
                }
            }
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

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

    // ---- parse_resize_frame ----

    #[test]
    fn parses_well_formed_resize_frame() {
        assert_eq!(parse_resize_frame(b"__resize:120x40"), Some((120, 40)));
    }

    #[test]
    fn rejects_non_resize_and_malformed_frames() {
        assert_eq!(parse_resize_frame(CLOSE_FRAME), None);
        assert_eq!(parse_resize_frame(b"ls\r"), None);
        assert_eq!(parse_resize_frame(b"__resize:"), None);
        assert_eq!(parse_resize_frame(b"__resize:bogus"), None);
        // 多一个 'x' 不该被宽容地截断
        assert_eq!(parse_resize_frame(b"__resize:1x2x3"), None);
    }

    // ---- wait_for_close ----

    #[test]
    fn close_frame_returns_true_without_waiting_the_full_delay() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(CLOSE_FRAME.to_vec()).unwrap();
        let mut dims = (80, 24);
        let start = Instant::now();
        assert!(wait_for_close(&rx, Duration::from_secs(30), &mut dims));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(dims, (80, 24));
    }

    #[test]
    fn no_close_returns_false_after_the_delay() {
        let (_tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let mut dims = (80, 24);
        assert!(!wait_for_close(&rx, Duration::from_millis(150), &mut dims));
    }

    #[test]
    fn resize_during_wait_is_kept_and_close_still_seen() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(b"__resize:100x30".to_vec()).unwrap();
        tx.send(CLOSE_FRAME.to_vec()).unwrap();
        let mut dims = (80, 24);
        assert!(wait_for_close(&rx, Duration::from_secs(30), &mut dims));
        // 尺寸帧不能被当成噪声丢掉，否则重连后 PTY 尺寸是旧的
        assert_eq!(dims, (100, 30));
    }

    #[test]
    fn plain_keystrokes_are_dropped() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(b"ls\r".to_vec()).unwrap();
        tx.send(CLOSE_FRAME.to_vec()).unwrap();
        let mut dims = (80, 24);
        assert!(wait_for_close(&rx, Duration::from_secs(30), &mut dims));
        assert_eq!(dims, (80, 24));
    }

    #[test]
    fn disconnected_input_channel_counts_as_close() {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        drop(tx);
        let mut dims = (80, 24);
        assert!(wait_for_close(&rx, Duration::from_secs(30), &mut dims));
    }

    // ---- drain_pending ----

    #[test]
    fn drain_returns_false_when_nothing_is_buffered() {
        let (_tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let mut dims = (80, 24);
        assert!(!drain_pending(&rx, &mut dims));
    }

    #[test]
    fn drain_consumes_the_whole_backlog_resize_included() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(b"__resize:90x20".to_vec()).unwrap();
        tx.send(b"ls\r".to_vec()).unwrap();
        tx.send(CLOSE_FRAME.to_vec()).unwrap();
        let mut dims = (80, 24);
        assert!(drain_pending(&rx, &mut dims));
        assert_eq!(dims, (90, 20));
    }
}
