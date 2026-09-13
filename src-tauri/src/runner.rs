use std::io::Read;
use std::process::Command as StdCommand;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use session_core::backoff::Backoff;
use session_core::command::{build_probe_argv, build_session_argv, destination, shell_quote, SshTarget};
use session_core::exit::{classify_ssh, SshOutcome};
use session_core::reconnect::{
    classify_unexpected_exit, decide, drain_pending, parse_resize_frame, wait_for_close,
    RetryDecision, CLOSE_FRAME,
};
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
    cols: u16,
    rows: u16,
    input_rx: mpsc::Receiver<Vec<u8>>,
    kill_remote_on_close: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut backoff = Backoff::default();
    // I3: 首次探测必然 Ok(false)（会话尚不存在），不应发"原会话已不在"提示
    let mut first_attempt = true;
    // 尺寸由 resize 控制帧更新；退避期间也必须接住，否则重连出来的 PTY 用旧尺寸
    let mut dims = (cols, rows);

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
                        // 退避等待必须能被 __close 打断：网络类失败会无限重连，
                        // 用 sleep 的话这一轮就再也收不到用户的关闭请求
                        if wait_for_close(&input_rx, delay, &mut dims) {
                            finish_close(&tx, &target, &session, &kill_remote_on_close);
                            return;
                        }
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

        // probe_session 是阻塞的（最长 ConnectTimeout=10s），期间用户可能已经点了关闭。
        // 不把积压的控制帧收掉，就会在关闭之后又 spawn 一个 PTY。
        if drain_pending(&input_rx, &mut dims) {
            finish_close(&tx, &target, &session, &kill_remote_on_close);
            return;
        }

        let argv = build_session_argv(&target, &session, &agent_cmd);
        let mut pty = match PtySession::spawn(&argv, dims.0, dims.1) {
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
                    if frame.as_slice() == CLOSE_FRAME {
                        closed = true;
                        break;
                    }
                    if let Some(size) = parse_resize_frame(&frame) {
                        // 尺寸没变就什么都不做：ResizeObserver 抖动会重复发同样的尺寸，
                        // 每次都 resize + 注入一遍 refresh 序列，纯属浪费，还可能把
                        // 序列漏进画面。
                        if size != dims {
                            dims = size;
                            let _ = pty.resize(dims.0, dims.1);
                            // tmux 对 resize 只发差量重绘，首帧又是按 spawn 时的初始
                            // 尺寸画的 —— 不强制全量重绘，旧尺寸的错字会永久留在屏上
                            let _ = pty.write(session_core::reconnect::TMUX_REFRESH);
                        }
                        continue;
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
            // R11.2/R11.3: 用户关闭路径也必须 wait 收割子进程，否则留下僵尸
            let _ = pty.wait();
            finish_close(&tx, &target, &session, &kill_remote_on_close);
            return;
        }

        // 进程意外结束：stderr 已并入 PTY 流，只能按 退出码+在线时长 启发式分类（R17）
        let code = pty.wait().ok();
        let kind = classify_unexpected_exit(code, connected_at.elapsed());
        let _ = tx.send(RunnerMsg::State(SessionState::Retrying));
        // decide() 已经给出退避时长，不要再调 next_delay()（R7）
        match decide(kind, &mut backoff) {
            RetryDecision::Retry(delay) => {
                if wait_for_close(&input_rx, delay, &mut dims) {
                    finish_close(&tx, &target, &session, &kill_remote_on_close);
                    return;
                }
            }
            RetryDecision::GiveUp => {
                let _ = tx.send(RunnerMsg::State(SessionState::Exited));
                return;
            }
        }
    }
}

/// 远端销毁 tmux 会话 —— 只在用户显式要求（kill_remote=true）时走这里。
/// 尽力而为：失败无非是远端会话多留一会儿，不该影响本地关闭。
fn kill_remote_session(target: &SshTarget, session: &str) {
    let remote = format!("tmux kill-session -t {}", shell_quote(session));
    let mut argv = vec!["-o".to_string(), "BatchMode=yes".to_string()];
    argv.extend(target.extra_ssh_args.iter().cloned());
    argv.push(destination(target));
    argv.push(remote);
    let mut kill_cmd = StdCommand::new("ssh");
    kill_cmd.args(&argv);
    let _ = hide_console(&mut kill_cmd).status();
}

/// 关闭收尾：按需销毁远端会话，再发 `Closed`。
/// 不碰 PTY —— 有 PTY 的路径必须自己先 kill + wait（ledger 2/3：PtySession 没有 Drop）。
fn finish_close(
    tx: &Sender<RunnerMsg>,
    target: &SshTarget,
    session: &str,
    kill_remote_on_close: &std::sync::atomic::AtomicBool,
) {
    if kill_remote_on_close.load(Ordering::SeqCst) {
        kill_remote_session(target, session);
    }
    let _ = tx.send(RunnerMsg::State(SessionState::Closed));
}
