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
    Output(String),
    State(SessionState),
    Notice(String),
}

/// Windows 下从 GUI 进程启动控制台程序（ssh）会分配一个新控制台窗口，
/// 于是每次探测/重试/列目录都会闪一个 cmd 窗口。加 CREATE_NO_WINDOW 抑制它。
/// 其他平台为空操作。
pub fn hide_console(cmd: &mut StdCommand) -> &mut StdCommand {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// probe 远端会话是否已存在。返回 Ok(true)=已存在，Ok(false)=不存在。
pub fn probe_session(target: &SshTarget, session: &str) -> Result<bool, SshOutcome> {
    let argv = build_probe_argv(target, session);
    let mut cmd = StdCommand::new(&argv[0]);
    cmd.args(&argv[1..]);
    match hide_console(&mut cmd).output() {
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
    // I3: 首次探测必然 Ok(false)（会话尚不存在），不应发"原会话已不在"提示
    let mut first_attempt = true;

    loop {
        let is_first_probe = first_attempt;
        first_attempt = false;

        // 先判断会话是否还在，用于提示"原会话已不在，已新建"
        match probe_session(&target, &session) {
            Ok(true) => {}
            Ok(false) => {
                if !is_first_probe {
                    let _ = tx.send(RunnerMsg::Notice(
                        "原会话已不在，已新建".to_string(),
                    ));
                }
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
                        // I2: 放弃前给出人话原因，供前端 notice 渲染
                        let reason = match kind {
                            SshOutcome::TmuxMissing => "服务端缺 tmux，请先安装：sudo dnf install tmux（或 sudo apt install tmux）".to_string(),
                            SshOutcome::Auth => "SSH 认证失败：请检查密钥/用户名".to_string(),
                            _ => "连接失败".to_string(),
                        };
                        let _ = tx.send(RunnerMsg::Notice(reason));
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
            // 保留不完整的 UTF-8 尾序列，等下一个读取块拼齐（I4）
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text =
                            session_core::transport::complete_utf8_prefix(&mut pending, &buf[..n]);
                        if out_tx.send(RunnerMsg::Output(text)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // 读完仍剩下的不完整序列：lossy 兜底，不静默丢弃
            if !pending.is_empty() {
                let _ = out_tx.send(RunnerMsg::Output(String::from_utf8_lossy(&pending).to_string()));
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
                let mut argv = vec!["-o".to_string(), "BatchMode=yes".to_string()];
                argv.extend(target.extra_ssh_args.iter().cloned());
                argv.push(destination(&target));
                argv.push(remote);
                let mut kill_cmd = StdCommand::new("ssh");
                kill_cmd.args(&argv);
                let _ = hide_console(&mut kill_cmd).status();
            }
            let _ = tx.send(RunnerMsg::State(SessionState::Closed));
            // R11.2/R11.3: 用户关闭路径也必须 wait 收割子进程，否则留下僵尸
            let _ = pty.wait();
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
