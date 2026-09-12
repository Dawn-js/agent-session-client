use serde::{Deserialize, Serialize};

use crate::command::SshTarget;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub hosts: Vec<HostConfig>,
    pub agents: Vec<AgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostConfig {
    pub name: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_ssh_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    pub id: String,
    pub label: String,
    pub cmd: String,
}

pub fn parse_config(json: &str) -> Result<Config, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn validate(cfg: &Config) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if cfg.hosts.is_empty() {
        errs.push("hosts: must not be empty".to_string());
    }
    if cfg.agents.is_empty() {
        errs.push("agents: must not be empty".to_string());
    }
    for (i, h) in cfg.hosts.iter().enumerate() {
        if h.name.trim().is_empty() {
            errs.push(format!("hosts[{i}].name: must not be empty"));
        }
        if h.host.trim().is_empty() {
            errs.push(format!("hosts[{i}].host: must not be empty"));
        }
    }
    for (i, a) in cfg.agents.iter().enumerate() {
        if a.id.trim().is_empty() {
            errs.push(format!("agents[{i}].id: must not be empty"));
        }
        if a.cmd.trim().is_empty() {
            errs.push(format!("agents[{i}].cmd: must not be empty"));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

impl Config {
    pub fn to_ssh_target(&self, host_name: &str) -> Option<SshTarget> {
        self.hosts.iter().find(|h| h.name == host_name).map(|h| SshTarget {
            host: h.host.clone(),
            user: h.user.clone(),
            extra_ssh_args: h.extra_ssh_args.clone(),
        })
    }
}

/// 把界面给的主机标识解析成连接目标，**不需要已有配置**。
///
/// 先在已加载的配置里按 `host.name` 查；查不到就把它当成 ssh 别名原样交给系统
/// ssh（`~/.ssh/config` 会去解释 HostName/User/Port）。
///
/// 首次启动时还没有配置文件，界面上列的正是 `~/.ssh/config` 的别名 ——
/// 探测必须走别名这条路，否则「选一台服务器开始」只能退化成猜 agent
/// （把注册表全集写进配置，点没装的 agent 必然挂）。
pub fn resolve_target(config: Option<&Config>, host_or_alias: &str) -> SshTarget {
    config
        .and_then(|cfg| cfg.to_ssh_target(host_or_alias))
        .unwrap_or_else(|| SshTarget {
            host: host_or_alias.to_string(),
            user: None,
            extra_ssh_args: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
      "hosts": [{ "name": "main", "host": "10.0.0.1", "user": "ubuntu" }],
      "agents": [{ "id": "hermes", "label": "Hermes", "cmd": "hermes chat" }]
    }"#;

    #[test]
    fn parses_valid_config() {
        let cfg = parse_config(GOOD).unwrap();
        assert_eq!(cfg.hosts[0].name, "main");
        assert_eq!(cfg.agents[0].cmd, "hermes chat");
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn missing_required_field_reports_serde_error() {
        let err = parse_config(r#"{ "hosts": [] }"#).unwrap_err();
        assert!(err.contains("agents"), "unexpected error: {err}");
    }

    #[test]
    fn validate_names_the_offending_field() {
        let cfg = parse_config(r#"{
          "hosts": [{ "name": "", "host": "h" }],
          "agents": [{ "id": "a", "label": "A", "cmd": "  " }]
        }"#).unwrap();
        let errs = validate(&cfg).unwrap_err();
        assert_eq!(errs, vec![
            "hosts[0].name: must not be empty".to_string(),
            "agents[0].cmd: must not be empty".to_string(),
        ]);
    }

    #[test]
    fn resolves_ssh_target_by_host_name() {
        let cfg = parse_config(GOOD).unwrap();
        let t = cfg.to_ssh_target("main").unwrap();
        assert_eq!(t.host, "10.0.0.1");
        assert_eq!(t.user.as_deref(), Some("ubuntu"));
        assert!(t.extra_ssh_args.is_empty());
        assert!(cfg.to_ssh_target("nope").is_none());
    }

    #[test]
    fn resolves_saved_host_through_config() {
        let cfg = parse_config(GOOD).unwrap();
        let t = resolve_target(Some(&cfg), "main");
        assert_eq!(t.host, "10.0.0.1");
        assert_eq!(t.user.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn falls_back_to_raw_alias_when_no_config_exists() {
        // 首次启动还没有配置文件。此时界面上列的是 ~/.ssh/config 的别名，
        // 探测必须能按别名直接连 —— 否则首启只能靠猜 agent。
        let t = resolve_target(None, "main");
        assert_eq!(t.host, "main");
        assert_eq!(t.user, None);
        assert!(t.extra_ssh_args.is_empty());
    }

    #[test]
    fn falls_back_to_raw_alias_when_config_lacks_the_name() {
        let cfg = parse_config(GOOD).unwrap();
        let t = resolve_target(Some(&cfg), "some-alias");
        assert_eq!(t.host, "some-alias");
        assert_eq!(t.user, None);
    }

    #[test]
    fn serialize_roundtrip_preserves_config_and_skips_empty_fields() {
        let cfg = parse_config(GOOD).unwrap();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        assert!(!json.contains("extra_ssh_args"), "empty vec must be skipped: {json}");
        assert_eq!(parse_config(&json).unwrap(), cfg);
    }
}
