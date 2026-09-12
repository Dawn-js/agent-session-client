//! 探测远端装了哪些已知 agent CLI。
//!
//! 只依赖系统 `ssh` + `command -v`，不往服务器上装任何东西（架构不变量）。
//! 刻意不改 PATH：判断用的就是会话启动时同一个非登录 shell 环境 ——
//! 能 `command -v` 到的，才说明写进配置的 `agents[].cmd` 真能跑起来。

pub struct KnownAgent {
    pub id: &'static str,
    pub label: &'static str,
    /// 在远端 PATH 上探测的可执行文件名
    pub bin: &'static str,
    /// 写进配置 `agents[].cmd` 的启动命令
    pub cmd: &'static str,
}

/// 已知 agent 注册表。支持新 agent 只需加一行。
pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent { id: "hermes", label: "Hermes Agent", bin: "hermes", cmd: "hermes chat" },
    KnownAgent { id: "dsh", label: "DeepSeek Harness", bin: "dsh", cmd: "dsh" },
];

/// 一条 ssh 命令探完注册表里所有 bin，命中的按行打印 bin 名。
///
/// 结尾的 `; true` 不能省：`for` 的退出码取最后一次执行，若最后一个 bin 没装，
/// 整条命令就是 1，而长驻通道把非零退出码当失败（filechan.rs）。bin 全来自本文件常量。
pub fn build_probe_agents_cmd() -> String {
    let bins: Vec<&str> = KNOWN_AGENTS.iter().map(|a| a.bin).collect();
    format!(
        "for b in {}; do command -v \"$b\" >/dev/null 2>&1 && echo \"$b\"; done; true",
        bins.join(" ")
    )
}

/// 解析探测输出：只认注册表里的 bin，其余行（stderr 被并进 stdout、shell 噪声）忽略。
/// 按注册表顺序返回并去重。
pub fn parse_probe_agents_output(out: &str) -> Vec<&'static KnownAgent> {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    KNOWN_AGENTS.iter().filter(|a| lines.contains(&a.bin)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_cmd_lists_every_registry_bin_and_never_fails() {
        let cmd = build_probe_agents_cmd();
        for agent in KNOWN_AGENTS {
            assert!(cmd.contains(agent.bin), "{} missing from: {cmd}", agent.bin);
        }
        assert!(cmd.contains("command -v"));
        // 最后一个 bin 没装时 for 的退出码是 1，会被长驻通道当成命令失败
        assert!(cmd.ends_with("; true"), "must force exit 0: {cmd}");
    }

    #[test]
    fn parses_installed_bins() {
        let got = parse_probe_agents_output("hermes\ndsh\n");
        assert_eq!(got.iter().map(|a| a.id).collect::<Vec<_>>(), vec!["hermes", "dsh"]);
    }

    #[test]
    fn ignores_unknown_and_noisy_lines() {
        // filechan 把远端 stderr 并进了 stdout，噪声必须容忍
        let got = parse_probe_agents_output("bash: warning: setlocale\nnope\n  hermes  \n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "hermes");
    }

    #[test]
    fn empty_output_means_nothing_installed() {
        assert!(parse_probe_agents_output("").is_empty());
    }

    #[test]
    fn output_order_is_registry_order_and_deduped() {
        let got = parse_probe_agents_output("dsh\nhermes\nhermes\n");
        assert_eq!(got.iter().map(|a| a.id).collect::<Vec<_>>(), vec!["hermes", "dsh"]);
    }

    #[test]
    fn discovered_agents_carry_a_usable_launch_command() {
        for agent in parse_probe_agents_output("hermes\ndsh\n") {
            assert!(!agent.cmd.trim().is_empty(), "{} has no cmd", agent.id);
            assert!(!agent.label.trim().is_empty());
        }
    }
}
