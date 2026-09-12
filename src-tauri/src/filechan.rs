//! 文件面板专用的长驻 ssh 通道。
//!
//! 列一次目录要付一次完整的 ssh 握手：实测端到端 3-5s，而其中真正传数据
//! 只有 0.8s。所以握手只做一次，之后的每次列目录只是一个 RTT。
//!
//! 为什么不用 ControlMaster：本机实测（Git for Windows 的 ssh 10.3p1）master
//! 能起来（`ssh -O check` 报 running），但每次请求 session 都
//! `read from master failed: Connection reset by peer` 后退回新建连接，
//! 耗时和不复用一样。MSYS 的 socket 层和 Windows 原生 socket 之间的 mux 不通。
//!
//! 协议：一行一条命令，远端 `eval` 之后回一段输出，以独占一行的
//! `__END__<退出码>` 结束。命令全部由本程序构造（路径已 shell_quote），
//! 不接受任何外部输入。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use session_core::command::{build_exec_argv, SshTarget};

use crate::runner::hide_console;

/// 通道空闲多久就关掉：不留一个没人用的 ssh 挂在后台。
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// 单条命令的等待上限。超时后响应流是脏的，整个通道会被丢弃重建。
const CMD_TIMEOUT: Duration = Duration::from_secs(30);

const END_MARKER: &str = "__END__";

/// 远端循环。`exec 2>&1` 把 stderr 并进 stdout，否则远端报错（比如目录不存在）
/// 走的是 stderr，拿不回来。
const REMOTE_LOOP: &str = "exec 2>&1\n\
    while IFS= read -r line; do\n\
    eval \"$line\"\n\
    printf '__END__%s\\n' \"$?\"\n\
    done\n";

struct Resp {
    out: String,
    code: i32,
}

pub struct FileChan {
    host: String,
    stdin: ChildStdin,
    rx: Receiver<Resp>,
    child: Child,
    last_used: Instant,
}

impl FileChan {
    pub fn spawn(target: &SshTarget, host: &str) -> Result<Self, String> {
        let argv = build_exec_argv(target, REMOTE_LOOP);
        let mut child = hide_console(
            Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map_err(|e| format!("启动 ssh 失败: {e}"))?;

        let stdin = child.stdin.take().ok_or("拿不到 ssh stdin")?;
        let stdout = child.stdout.take().ok_or("拿不到 ssh stdout")?;
        let (tx, rx) = mpsc::channel::<Resp>();
        std::thread::spawn(move || read_loop(stdout, tx));

        Ok(FileChan { host: host.to_string(), stdin, rx, child, last_used: Instant::now() })
    }

    /// 换 host、或空闲太久的通道不能再用。
    pub fn usable(&self, host: &str) -> bool {
        self.host == host && self.last_used.elapsed() < IDLE_TIMEOUT
    }

    /// 跑一条远端命令，返回它的 stdout+stderr。非零退出码算失败。
    pub fn run(&mut self, cmd: &str) -> Result<String, String> {
        self.last_used = Instant::now();
        writeln!(self.stdin, "{cmd}").map_err(|e| format!("写入 ssh 失败: {e}"))?;
        self.stdin.flush().map_err(|e| format!("刷新 ssh 失败: {e}"))?;

        match self.rx.recv_timeout(CMD_TIMEOUT) {
            Ok(resp) if resp.code == 0 => Ok(resp.out),
            // stderr 已经并进 stdout，出错时输出本身就是人话
            Ok(resp) => {
                let msg = resp.out.trim();
                Err(if msg.is_empty() {
                    format!("远端命令退出码 {}", resp.code)
                } else {
                    msg.to_string()
                })
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err("远端命令超时".to_string()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("ssh 通道已断开".to_string()),
        }
    }
}

impl Drop for FileChan {
    fn drop(&mut self) {
        // 不 kill，打包后的应用退出后 ssh 会留成孤儿进程
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 按行读，攒到 `__END__` 行就把整段发出去。通道断开时剩下的半截直接丢掉，
/// 由调用方重建通道重跑。
fn read_loop(stdout: ChildStdout, tx: Sender<Resp>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut buf = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => match line.strip_prefix(END_MARKER) {
                Some(code) => {
                    let resp = Resp { out: std::mem::take(&mut buf), code: code.trim().parse().unwrap_or(-1) };
                    if tx.send(resp).is_err() {
                        break;
                    }
                }
                None => buf.push_str(&line),
            },
            Err(_) => break,
        }
    }
}
