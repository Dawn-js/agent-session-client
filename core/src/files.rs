//! 远端目录列表：用 GNU `find` 拿一行一条的可解析输出。
//!
//! 不用 `ls` —— 它的输出受 locale、颜色开关、列宽影响，解析起来脆。
//! `-printf` 的输出格式由我们指定，服务器是 Ubuntu（GNU findutils），够用。

use crate::command::shell_quote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// 远端 `pwd` 解析出的**规范绝对路径**。
    /// 前端传 `~` 或 `/home/ubuntu/..` 进来，回来的都是 `/home/ubuntu` —— 面包屑靠它。
    pub dir: String,
    pub entries: Vec<Entry>,
}

/// 引用一个目录路径供 shell 使用。
///
/// 不能直接 `shell_quote`：它会把 `~` 一起塞进单引号里，shell 就不再展开它，
/// `cd '~'` 会去找一个**字面名叫 `~` 的目录**并报 `No such file or directory`。
/// 所以 `~` 要替换成 `$HOME` 并放在引号**外面**（shell 里 `$HOME'/x'` 会拼接成一个词）。
fn quote_dir(dir: &str) -> String {
    match dir.strip_prefix('~') {
        Some("") => "$HOME".to_string(),
        Some(rest) if rest.starts_with('/') => format!("$HOME{}", shell_quote(rest)),
        // `~user` 这种形式不处理：让它在远端响亮地失败，比猜错强
        _ => shell_quote(dir),
    }
}

/// 列 `dir` 下一层的远端命令。
///
/// `cd && pwd` 而不是直接 `find <dir>`：`~` 和 `..` 只有远端 shell 展开得了，
/// 让远端把规范路径报回来，前端就不用猜。`pwd` 那行没有制表符，
/// 会被 `parse_find_output` 自然丢掉，所以两段输出可以混在一条流里。
///
/// `-mindepth 1` 排除 `.` 自身。`%Y` 而非 `%y`：符号链接按**目标**类型判定，
/// 否则指向目录的链接会被当成普通文件、点不进去（`~` 下这种链接很常见）。
pub fn build_list_cmd(dir: &str) -> String {
    format!(
        "cd {} && pwd && find . -mindepth 1 -maxdepth 1 -printf '%Y\\t%s\\t%f\\n'",
        quote_dir(dir)
    )
}

/// 解析 `build_list_cmd` 的完整输出：首行是 `pwd`，其余是条目。
pub fn parse_listing(out: &str) -> Listing {
    match out.split_once('\n') {
        Some((dir, rest)) => Listing {
            dir: dir.trim().to_string(),
            entries: parse_find_output(rest),
        },
        None => Listing {
            dir: out.trim().to_string(),
            entries: Vec::new(),
        },
    }
}

/// 解析条目行。无法解析的行直接丢弃 —— 单个坏行不该让整个列表失败。
pub fn parse_find_output(out: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = out
        .lines()
        .filter_map(|line| {
            // splitn(3) 让第 3 段把剩余制表符全吃掉，名字里的制表符不会被截断
            let mut parts = line.splitn(3, '\t');
            let kind = parts.next()?;
            let size = parts.next()?.parse().ok()?;
            let name = parts.next()?;
            if name.is_empty() {
                return None;
            }
            Some(Entry {
                name: name.to_string(),
                // 只有 'd' 当目录；断链是 'N'，其余按文件处理
                is_dir: kind == "d",
                size,
            })
        })
        .collect();

    // 目录在前，再按名字（忽略大小写）——列表顺序在这里定一次，前端不再排序
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// 把目录和条目名拼成绝对路径。`dir` 为 `/` 时不产生 `//`。
pub fn join_path(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
}

/// 列 agent 已安装 skill 的远端命令。
///
/// 层级不固定：有的在 `skills/<name>/SKILL.md`，有的在 `skills/<category>/<name>/SKILL.md`
/// （实测 5 个单层 + 67 个双层）。所以拿 `SKILL.md` 当唯一标识去找，而不是假设深度。
///
/// 输出三列：**目录名 / frontmatter 的 name / description**。
/// name 可能缺失，解析层用目录名兜底，所以两列都得给。
pub fn build_skills_cmd(dir: &str) -> String {
    format!(
        "find {} -maxdepth 3 -name SKILL.md 2>/dev/null | while read -r f; do \
         n=$(sed -n 's/^name:[[:space:]]*//p' \"$f\" | head -1); \
         d=$(sed -n 's/^description:[[:space:]]*//p' \"$f\" | head -1); \
         printf '%s\\t%s\\t%s\\n' \"$(basename \"$(dirname \"$f\")\")\" \"$n\" \"$d\"; \
         done",
        quote_dir(dir)
    )
}

pub fn parse_skills_output(out: &str) -> Vec<Skill> {
    let mut skills: Vec<Skill> = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let dir_name = parts.next()?.trim();
            let declared = parts.next().unwrap_or("").trim();
            let description = parts.next().unwrap_or("").trim();
            // frontmatter 里没写 name 的，退回目录名
            let name = unquote(if declared.is_empty() { dir_name } else { declared });
            if name.is_empty() {
                return None;
            }
            Some(Skill { name, description: unquote(description) })
        })
        .collect();

    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

/// YAML 里字符串常带成对引号，去掉。只剥 ASCII 引号，所以按字节切是安全的。
fn unquote(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_cmd_cds_then_pwds_then_finds() {
        assert_eq!(
            build_list_cmd("/home/ubuntu/my proj"),
            "cd '/home/ubuntu/my proj' && pwd && \
             find . -mindepth 1 -maxdepth 1 -printf '%Y\\t%s\\t%f\\n'"
        );
    }

    #[test]
    fn tilde_is_expanded_outside_the_quotes() {
        // 回归：`cd '~'` 会报 "No such file or directory" ——
        // 单引号阻止 shell 展开 ~，于是去找字面名叫 ~ 的目录。
        assert!(build_list_cmd("~").starts_with("cd $HOME && pwd"));
        // ~/x 要拼成一个词：shell 里 $HOME'/x' 是合法的
        assert!(build_list_cmd("~/proj").starts_with("cd $HOME'/proj' && pwd"));
        // 普通路径仍然整体加引号
        assert!(build_list_cmd("/tmp").starts_with("cd '/tmp' && pwd"));
        assert!(build_list_cmd("/a b/c").starts_with("cd '/a b/c' && pwd"));
    }

    #[test]
    fn listing_takes_first_line_as_resolved_dir() {
        let out = "/home/ubuntu\n\
                   d\t4096\tsrc\n\
                   f\t120\tREADME.md\n";
        let got = parse_listing(out);
        assert_eq!(got.dir, "/home/ubuntu");
        assert_eq!(
            got.entries,
            vec![
                Entry { name: "src".into(), is_dir: true, size: 4096 },
                Entry { name: "README.md".into(), is_dir: false, size: 120 },
            ]
        );
    }

    #[test]
    fn listing_with_no_entries_is_just_a_dir() {
        let got = parse_listing("/tmp\n");
        assert_eq!(got.dir, "/tmp");
        assert!(got.entries.is_empty());
    }

    #[test]
    fn pwd_line_never_leaks_into_entries() {
        // pwd 那行没有制表符，parse_find_output 必须丢掉它
        let got = parse_listing("/home/ubuntu\nf\t1\tonly-file\n");
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].name, "only-file");
    }

    #[test]
    fn sorts_dirs_first_then_case_insensitive_name() {
        let out = "f\t1\tzeta\n\
                   d\t1\tBeta\n\
                   d\t1\talpha\n\
                   f\t1\tAlpha\n";
        let names: Vec<_> = parse_find_output(out).into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["alpha", "Beta", "Alpha", "zeta"]);
    }

    #[test]
    fn symlink_to_dir_counts_as_dir() {
        // %Y 已把链接解析成目标类型
        assert!(parse_find_output("d\t1\tlink-to-dir\n")[0].is_dir);
        // 断链是 'N'，按文件处理而不是丢掉
        assert!(!parse_find_output("N\t1\tbroken\n")[0].is_dir);
    }

    #[test]
    fn drops_unparseable_lines_without_failing() {
        let out = "garbage\n\
                   f\tnotanumber\tbad\n\
                   \n\
                   f\t7\tok\n";
        let got = parse_find_output(out);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "ok");
    }

    #[test]
    fn keeps_names_with_tabs_in_them_intact() {
        // splitn(3) 保证第 3 段把剩余制表符全吃掉，名字不会被截断
        let got = parse_find_output("f\t3\tweird\tname\n");
        assert_eq!(got[0].name, "weird\tname");
    }

    #[test]
    fn join_path_avoids_double_slash() {
        assert_eq!(join_path("/home", "ubuntu"), "/home/ubuntu");
        assert_eq!(join_path("/", "home"), "/home");
    }

    #[test]
    fn skills_cmd_searches_by_skill_md_not_by_depth() {
        // 层级不固定（5 个单层 + 67 个双层），所以不能假设深度
        let cmd = build_skills_cmd("~/.hermes/skills");
        assert!(cmd.starts_with("find $HOME'/.hermes/skills' -maxdepth 3 -name SKILL.md"));
        // 目录名也要输出，供 name 缺失时兜底
        assert!(cmd.contains("basename"));
    }

    #[test]
    fn parses_skills_and_strips_yaml_quotes() {
        let out = "github\tgithub\t\"GitHub via gh CLI: PRs, issues.\"\n\
                   p5js\tp5js\t\"p5.js sketches: gen art, shaders.\"\n";
        let got = parse_skills_output(out);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "github");
        assert_eq!(got[0].description, "GitHub via gh CLI: PRs, issues.");
    }

    #[test]
    fn falls_back_to_dir_name_when_name_missing() {
        // frontmatter 没写 name 时用目录名兜底，而不是显示空白
        let got = parse_skills_output("my-skill\t\t\"desc\"\n");
        assert_eq!(got[0].name, "my-skill");
        assert_eq!(got[0].description, "desc");
    }

    #[test]
    fn handles_single_quotes_and_missing_description() {
        let got = parse_skills_output("a\ta\t'single quoted'\nb\tb\n");
        assert_eq!(got[0].description, "single quoted");
        assert_eq!(got[1].description, "");
    }

    #[test]
    fn skills_sorted_case_insensitively() {
        let names: Vec<_> = parse_skills_output("z\tzeta\t\nalpha\tAlpha\t\n")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["Alpha", "zeta"]);
    }
}
