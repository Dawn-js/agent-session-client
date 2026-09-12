//! 解析本机 `~/.ssh/config` 里的 `Host` 别名。
//!
//! 只做"把别名列出来给用户点"这一件事：**不做** `Include`/`Match` 展开、不做通配符匹配，
//! 也**不把**解析出的 `HostName`/`User`/`Port` 写进配置 —— 连接时把别名原样交给系统 `ssh`，
//! 由它自己解释 HostName/Port/ProxyJump/IdentityFile 等（README 承诺的"复用 `~/.ssh/config`"）。
//! 因此这里只收字符串、只吐结构，不碰文件系统（架构不变量：core 不依赖 Tauri/GUI）。
//!
//! 解析出的 `host_name`/`user`/`port`/`proxy_jump` 仅用于界面展示。

/// 一个可用的 `Host` 别名，以及同块内顺带解析出的展示信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostAlias {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub proxy_jump: Option<String>,
}

/// 关键字与值之间的分隔：空白，或单个 `=`（`HostName=x` 与 `HostName x` 等价）。
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let keyword = &line[..idx];
    let rest = line[idx..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    Some((keyword, rest))
}

/// 通配符/否定 pattern 不是真实主机名，列出来只会变成点了就错的按钮。
fn is_literal_alias(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.contains(['*', '?', '!'])
}

/// 解析 OpenSSH 客户端配置，按首次出现顺序返回其中的字面 `Host` 别名。
pub fn parse_ssh_config(text: &str) -> Vec<SshHostAlias> {
    // UTF-8 BOM 会让第一个关键字的 `trim()` 去不掉，必须显式剥。
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let mut out: Vec<SshHostAlias> = Vec::new();
    // 当前 Host 块指向 out 里的下标；`Host *` / `Match` 时清空，避免后续全局选项污染上一个别名。
    let mut section: Vec<usize> = Vec::new();

    // `lines()` 会去掉行尾的 `\r`，天然兼容 Windows CRLF —— 不要自己按 `\n` split。
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((keyword, value)) = split_keyword(line) else {
            continue;
        };

        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                section.clear();
                for pattern in value.split_whitespace() {
                    if !is_literal_alias(pattern) {
                        continue;
                    }
                    let idx = match out.iter().position(|h| h.alias == pattern) {
                        Some(existing) => existing,
                        None => {
                            out.push(SshHostAlias {
                                alias: pattern.to_string(),
                                host_name: None,
                                user: None,
                                port: None,
                                proxy_jump: None,
                            });
                            out.len() - 1
                        }
                    };
                    section.push(idx);
                }
            }
            // `Match` 之后的关键字不再属于任何 Host 块。
            "match" => section.clear(),
            "hostname" | "user" | "proxyjump" => {
                if value.is_empty() {
                    continue;
                }
                for &idx in &section {
                    let slot = match keyword.to_ascii_lowercase().as_str() {
                        "hostname" => &mut out[idx].host_name,
                        "user" => &mut out[idx].user,
                        _ => &mut out[idx].proxy_jump,
                    };
                    // ssh 语义是每参数 first-wins：只在还没设置时填。
                    if slot.is_none() {
                        *slot = Some(value.to_string());
                    }
                }
            }
            "port" => {
                let Ok(port) = value.parse::<u16>() else {
                    continue;
                };
                for &idx in &section {
                    if out[idx].port.is_none() {
                        out[idx].port = Some(port);
                    }
                }
            }
            _ => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(text: &str) -> Vec<String> {
        parse_ssh_config(text).into_iter().map(|h| h.alias).collect()
    }

    #[test]
    fn parses_a_basic_host_block() {
        let got = parse_ssh_config(
            "Host main\n  HostName 10.0.0.1\n  User ubuntu\n  Port 2222\n",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].alias, "main");
        assert_eq!(got[0].host_name.as_deref(), Some("10.0.0.1"));
        assert_eq!(got[0].user.as_deref(), Some("ubuntu"));
        assert_eq!(got[0].port, Some(2222));
        assert_eq!(got[0].proxy_jump, None);
    }

    #[test]
    fn one_line_may_declare_several_aliases_sharing_options() {
        let got = parse_ssh_config("Host a b c\n  HostName 10.0.0.9\n  User u\n");
        assert_eq!(aliases_of(&got), vec!["a", "b", "c"]);
        for h in &got {
            assert_eq!(h.host_name.as_deref(), Some("10.0.0.9"));
            assert_eq!(h.user.as_deref(), Some("u"));
        }
    }

    fn aliases_of(got: &[SshHostAlias]) -> Vec<&str> {
        got.iter().map(|h| h.alias.as_str()).collect()
    }

    #[test]
    fn host_star_is_skipped_and_does_not_leak_globals_into_the_previous_alias() {
        let got = parse_ssh_config(
            "Host main\n  HostName 10.0.0.1\nHost *\n  User global-user\n",
        );
        assert_eq!(aliases_of(&got), vec!["main"]);
        assert_eq!(
            got[0].user, None,
            "`Host *` 之后的全局选项不能算进上一个别名"
        );
    }

    #[test]
    fn wildcard_and_negated_patterns_are_dropped_but_literals_survive() {
        assert_eq!(aliases("Host *.example.com\n  User u\n"), Vec::<String>::new());
        let got = parse_ssh_config("Host *.example.com foo !bar\n  User u\n");
        assert_eq!(aliases_of(&got), vec!["foo"]);
        assert_eq!(got[0].user.as_deref(), Some("u"));
    }

    #[test]
    fn keyword_is_case_insensitive_and_accepts_equals_separator_and_tabs() {
        let got = parse_ssh_config("host main\n\tHOSTNAME=10.0.0.1\n\tUser=ubuntu\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].host_name.as_deref(), Some("10.0.0.1"));
        assert_eq!(got[0].user.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn line_start_comments_are_ignored() {
        let got = parse_ssh_config("# Host commented\nHost main\n  # User nobody\n  HostName 10.0.0.1\n");
        assert_eq!(aliases_of(&got), vec!["main"]);
        assert_eq!(got[0].user, None);
        assert_eq!(got[0].host_name.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn crlf_input_parses_the_same_as_lf() {
        let got = parse_ssh_config("Host main\r\n  HostName 10.0.0.1\r\n  User ubuntu\r\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].alias, "main");
        assert_eq!(got[0].host_name.as_deref(), Some("10.0.0.1"));
        assert_eq!(got[0].user.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn leading_utf8_bom_does_not_swallow_the_first_host() {
        let got = parse_ssh_config("\u{feff}Host main\n  HostName 10.0.0.1\n");
        assert_eq!(aliases_of(&got), vec!["main"]);
    }

    #[test]
    fn match_blocks_end_the_current_section() {
        let got = parse_ssh_config(
            "Host main\n  HostName 10.0.0.1\nMatch host foo\n  User nope\n",
        );
        assert_eq!(aliases_of(&got), vec!["main"]);
        assert_eq!(got[0].user, None);
    }

    #[test]
    fn repeated_alias_is_deduplicated_and_first_value_wins() {
        let got = parse_ssh_config(
            "Host a\n  HostName first\nHost a\n  HostName second\n  User late\n",
        );
        assert_eq!(aliases_of(&got), vec!["a"]);
        assert_eq!(got[0].host_name.as_deref(), Some("first"));
        // 第二次出现仍可补上尚未设置的字段。
        assert_eq!(got[0].user.as_deref(), Some("late"));
    }

    #[test]
    fn proxy_jump_is_captured_and_non_numeric_port_is_ignored() {
        let got = parse_ssh_config(
            "Host main\n  ProxyJump bastion\n  Port not-a-number\n",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(got[0].port, None);
    }

    #[test]
    fn empty_input_yields_no_hosts() {
        assert!(parse_ssh_config("").is_empty());
        assert!(parse_ssh_config("\n\n   \n").is_empty());
    }
}
