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
    fn serialize_roundtrip_preserves_config_and_skips_empty_fields() {
        let cfg = parse_config(GOOD).unwrap();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        assert!(!json.contains("extra_ssh_args"), "empty vec must be skipped: {json}");
        assert_eq!(parse_config(&json).unwrap(), cfg);
    }
}
