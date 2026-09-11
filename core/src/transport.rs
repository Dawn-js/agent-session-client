use std::io::{Read, Write};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
}

impl PtySession {
    pub fn spawn(argv: &[String], cols: u16, rows: u16) -> Result<Self, String> {
        let first = argv.first().ok_or_else(|| "empty argv".to_string())?;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(first);
        cmd.args(&argv[1..]);

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        Ok(Self { master: pair.master, child, reader: Some(reader), writer })
    }

    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader.take()
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer.write_all(bytes).map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())
    }

    pub fn kill(&mut self) -> Result<(), String> {
        self.child.kill().map_err(|e| e.to_string())
    }

    pub fn wait(&mut self) -> Result<i32, String> {
        let status = self.child.wait().map_err(|e| e.to_string())?;
        Ok(status.exit_code() as i32)
    }
}

/// Append `chunk` to `pending`, then drain and return the longest prefix of
/// `pending` that is valid UTF-8, leaving any incomplete trailing sequence
/// in `pending` for the next call. Unambiguously invalid sequences are
/// emitted as U+FFFD immediately so a lone bad byte never stalls the stream.
pub fn complete_utf8_prefix(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    out.push_str(&String::from_utf8_lossy(&pending[..valid]));
                    pending.drain(..valid);
                }
                match e.error_len() {
                    // 不完整结尾：等下一个 chunk
                    None => break,
                    // 确定非法序列：立即发出 U+FFFD，跳过这几个字节
                    Some(n) if n > 0 => {
                        out.push('\u{FFFD}');
                        pending.drain(..n);
                    }
                    // std 不会返回 Some(0)；防御性跳出避免死循环
                    Some(_) => break,
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::complete_utf8_prefix;

    // '中' = E4 B8 AD，3 字节 CJK 字符

    #[test]
    fn cjk_split_across_chunks() {
        let mut pending: Vec<u8> = Vec::new();
        assert_eq!(complete_utf8_prefix(&mut pending, &[0xE4, 0xB8]), "");
        assert_eq!(pending, vec![0xE4, 0xB8]);
        assert_eq!(complete_utf8_prefix(&mut pending, &[0xAD]), "中");
        assert!(pending.is_empty());
    }

    #[test]
    fn ascii_passes_through() {
        let mut pending: Vec<u8> = Vec::new();
        assert_eq!(complete_utf8_prefix(&mut pending, b"hello"), "hello");
        assert!(pending.is_empty());
    }

    #[test]
    fn lone_invalid_byte_emits_replacement_immediately() {
        let mut pending: Vec<u8> = Vec::new();
        assert_eq!(complete_utf8_prefix(&mut pending, &[0xFF]), "\u{FFFD}");
        assert!(pending.is_empty(), "invalid byte must not stall the stream");
    }

    #[test]
    fn invalid_byte_then_valid_data_recovered() {
        let mut pending: Vec<u8> = Vec::new();
        assert_eq!(complete_utf8_prefix(&mut pending, &[0xFF, b'a']), "\u{FFFD}a");
        assert!(pending.is_empty());
    }
}
