use std::io::Read;
use std::time::{Duration, Instant};

use session_core::transport::PtySession;

fn shim_path() -> String {
    format!("{}/tests/fixtures/fake-ssh.sh", env!("CARGO_MANIFEST_DIR"))
}

// Windows 下 CreateProcessW 无法直接执行 shebang 脚本（os error 193），
// 需要经由 bash 调起 fixture。查找顺序：
//   1. FAKE_SSH_BASH 环境变量（显式指定）
//   2. 常见 Git for Windows 安装路径
//   3. PATH 上的 bash.exe
#[cfg(windows)]
fn bash_on_windows() -> String {
    if let Ok(p) = std::env::var("FAKE_SSH_BASH") {
        if !p.is_empty() {
            return p;
        }
    }
    for p in [
        "C:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
    ] {
        if std::path::Path::new(p).exists() {
            return p.to_string();
        }
    }
    "bash.exe".to_string()
}

fn argv() -> Vec<String> {
    let mut cmd = Vec::new();
    #[cfg(windows)]
    cmd.push(bash_on_windows());
    cmd.push(shim_path());
    cmd
}

fn hang_argv() -> Vec<String> {
    let mut cmd = argv();
    cmd.push("hang".into());
    cmd
}

#[test]
fn spawns_reads_and_exits_zero() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    let mut reader = pty.take_reader().expect("reader");

    // 读满若干字节或超时
    let mut buf = vec![0u8; 256];
    let mut collected = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !String::from_utf8_lossy(&collected).contains("line-two") {
        let n = reader.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&collected);
    assert!(text.contains("FAKE-SSH-BANNER"), "got: {text}");
    assert!(text.contains("line-two"), "got: {text}");

    let code = pty.wait().expect("wait");
    assert_eq!(code, 0);
}

#[test]
fn writes_stdin_to_child() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    pty.write(b"hello\n").expect("write");
    let mut reader = pty.take_reader().expect("reader");
    let mut buf = vec![0u8; 256];
    let _ = reader.read(&mut buf);
    // 只要写入不报错即可（shim 走 PTY echo，具体回显不强制断言）
    let _ = pty.kill();
}

#[test]
fn kill_terminates_hung_child() {
    let mut pty = PtySession::spawn(&hang_argv(), 80, 24).expect("spawn");
    let _ = pty.take_reader();
    let _ = pty.kill();
    let start = Instant::now();
    let _ = pty.wait();
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn resize_after_spawn_succeeds() {
    let mut pty = PtySession::spawn(&argv(), 80, 24).expect("spawn");
    pty.resize(120, 40).expect("resize");
    let _ = pty.kill();
}
