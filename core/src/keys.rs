//! Input policy for terminal keystrokes forwarded to the remote agent.

/// Control characters that agents commonly treat as "quit":
/// Ctrl+C (ETX), Ctrl+D (EOT), Ctrl+Z (SUB), Ctrl+\ (FS).
///
/// Forwarding these lets an accidental keypress kill the agent, which also
/// destroys its tmux session — after that there is nothing left to reconnect
/// to. They are therefore dropped before reaching the PTY.
pub const QUIT_CONTROL_CHARS: [char; 4] = ['\u{3}', '\u{4}', '\u{1a}', '\u{1c}'];

/// Remove quit control characters while leaving everything else untouched
/// (regular text, pasted content, arrow keys / escape sequences).
pub fn strip_quit_keys(data: &str) -> String {
    data.chars()
        .filter(|c| !QUIT_CONTROL_CHARS.contains(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_is_stripped() {
        assert_eq!(strip_quit_keys("abc\u{3}def"), "abcdef");
    }

    #[test]
    fn ctrl_d_and_z_and_backslash_are_stripped() {
        assert_eq!(strip_quit_keys("\u{4}\u{1a}\u{1c}"), "");
    }

    #[test]
    fn ordinary_text_passes_through() {
        assert_eq!(strip_quit_keys("hermes chat\n"), "hermes chat\n");
    }

    #[test]
    fn escape_sequences_are_preserved() {
        // ESC (0x1b) 是方向键/转义序列的一部分，必须保留
        assert_eq!(strip_quit_keys("\u{1b}[A"), "\u{1b}[A");
    }

    #[test]
    fn mixed_input_keeps_text_and_arrows() {
        assert_eq!(strip_quit_keys("a\u{3}\u{1b}[B\u{4}b"), "a\u{1b}[Bb");
    }
}
