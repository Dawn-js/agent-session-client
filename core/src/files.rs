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
        shell_quote(dir)
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
}
