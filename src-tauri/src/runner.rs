use std::io::Read;
use std::process::Command as StdCommand;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use session_core::backoff::Backoff;
use session_core::command::{build_probe_argv, build_session_argv, destination, shell_quote, SshTarget};
use session_core::exit::{classify_ssh, SshOutcome};
use session_core::reconnect::{classify_unexpected_exit, decide, RetryDecision};
use session_core::session::{transition, SessionEvent, SessionState};
use session_core::transport::PtySession;

pub enum RunnerMsg {
    Output(Vec<u8>),
    State(SessionState),
    Notice(String),
}

/// probe 远端会话是否已存在。返回 Ok(true)=已存在，Ok(false)=不存在。
pub fn probe_session(target: &SshTarget, session: &str) -> Result<bool, SshOutcome> {
    let argv = build_probe_argv(target, session);
    match StdCommand::new(&argv[0]).args(&argv[1..]).output() {
        Ok(out) if out.status.success() => Ok(true),
        Ok(out) => {
            if out.status.code() == Some(1) {
                Ok(false) // tmux has-session 返回 1 = 会话不存在
            } else {
                Err(classify_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)))
            }
        }
        Err(e) => Err(classify_ssh(None, &e.to_string())),
    }
}

/// 运行一个会话直到用户关闭（或不可重试的失败）。事件经 `tx` 发出。
pub fn run_session(
    tx: Sender<RunnerMsg>,
    target: SshTarget,
    session: String,
    agent_cmd: String,
    mut cols: u16,
    mut rows: u16,
    input_rx: mpsc::Receiver<Vec<u8>>,
    kill_remote_on_close: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut backoff = Backoff::default();

    loop {
        // 先判断会话是否还在，用于提示"原会话已不在，已新建"
        match probe_session(&target, &session) {
            Ok(true) => {}
            Ok(false) => {
                let _ = tx.send(RunnerMsg::Notice(
                    "原会话已不在，已新建".to_string(),
                ));
            }
            Err(kind) => {
                // decide() 已经给出退避时长，不要再调 next_delay()（R7）
                match decide(kind, &mut backoff) {
                    RetryDecision::Retry(delay) => {
                        let _ = tx.send(RunnerMsg::State(SessionState::Retrying));
                        thread::sleep(delay);
                        continue;
                    }
                    RetryDecision::GiveUp => {
                        let _ = tx.send(RunnerMsg::State(transition(
                            SessionState::Connecting,
                            SessionEvent::GiveUp,
                        )));
                        return;
                    }
                }
            }
        }

        let argv = build_session_argv(&target, &session, &agent_cmd);
        let mut pty = match PtySession::spawn(&argv, cols, rows) {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(RunnerMsg::Notice(format!("spawn failed: {e}")));
                return;
            }
        };
        let _ = tx.send(RunnerMsg::State(SessionState::Connected));
        backoff.reset();
        // R17: 记录连接建立时刻，用于意外退出的 uptime 启发式分类
        let connected_at = Instant::now();

        let mut reader = match pty.take_reader() {
            Some(r) => r,
            None => {
                // R11: 无 Drop，任何退出路径都要 kill + wait
                let _ = pty.kill();
                let _ = pty.wait();
                return;
            }
        };

        let out_tx = tx.clone();
        let reader_handle = thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if out_tx.send(RunnerMsg::Output(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // 转发用户输入；`__resize:<cols>x<rows>` 控制帧用于尺寸变化
        let mut closed = false;
        loop {
            match input_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(frame) => {
                    if let Some(dims) = frame.strip_prefix(b"__resize:") {
                        if let Ok(s) = std::str::from_utf8(dims) {
                            if let Some((c, r)) = s.split_once('x') {
                                if let (Ok(c), Ok(r)) = (c.parse(), r.parse()) {
                                    cols = c;
                                    rows = r;
                                    let _ = pty.resize(cols, rows);
                                }
                            }
                        }
                        continue;
                    }
                    if frame == b"__close" {
                        closed = true;
                        break;
                    }
                    let _ = pty.write(&frame);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if reader_handle.is_finished() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }

        // R11: 先 kill 再 wait（wait 会无限阻塞），每条退出路径都清理子进程
        let _ = pty.kill();
        let _ = reader_handle.join();

        if closed {
            if kill_remote_on_close.load(Ordering::SeqCst) {
                let remote = format!("tmux kill-session -t {}", shell_quote(&session));
                let _ = StdCommand::new("ssh")
                    .args(["-o", "BatchMode=yes", &destination(&target), &remote])
                    .status();
            }
            let _ = tx.send(RunnerMsg::State(SessionState::Closed));
            return;
        }

        // 进程意外结束：stderr 已并入 PTY 流，只能按 退出码+在线时长 启发式分类（R17）
        let code = pty.wait().ok();
        let kind = classify_unexpected_exit(code, connected_at.elapsed());
        let _ = tx.send(RunnerMsg::State(SessionState::Retrying));
        // decide() 已经给出退避时长，不要再调 next_delay()（R7）
        match decide(kind, &mut backoff) {
            RetryDecision::Retry(delay) => thread::sleep(delay),
            RetryDecision::GiveUp => {
                let _ = tx.send(RunnerMsg::State(SessionState::Exited));
                return;
            }
        }
    }
}
