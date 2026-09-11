//! Config file discovery: resolve the first usable config from a prioritized
//! candidate list, and provide the first-run template.

use std::path::PathBuf;

use crate::config::{parse_config, validate, Config};

/// Outcome of searching a prioritized list of candidate config paths.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    Loaded {
        path: PathBuf,
        config: Config,
    },
    NotFound {
        searched: Vec<PathBuf>,
    },
    Invalid {
        path: PathBuf,
        errors: Vec<String>,
    },
}

/// Load config from the first existing candidate.
///
/// The order is significant: the first file that exists wins, even if it fails
/// validation. This is deliberate — a broken user config must be reported
/// (with its path) rather than silently skipped in favour of a bundled example.
pub fn load_from_candidates(candidates: &[PathBuf]) -> LoadOutcome {
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let errors = match std::fs::read_to_string(path) {
            Ok(text) => match parse_config(&text) {
                Ok(cfg) => match validate(&cfg) {
                    Ok(()) => {
                        return LoadOutcome::Loaded {
                            path: path.clone(),
                            config: cfg,
                        }
                    }
                    Err(errors) => errors,
                },
                Err(error) => vec![error],
            },
            Err(error) => vec![error.to_string()],
        };
        return LoadOutcome::Invalid {
            path: path.clone(),
            errors,
        };
    }

    LoadOutcome::NotFound {
        searched: candidates.to_vec(),
    }
}

/// Deduplicate paths while preserving order. Common case: the current working
/// directory and the executable directory are identical.
pub fn unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

/// JSON template written by the "generate example config" first-run action.
///
/// Placeholders are intentionally non-empty so the template passes validation;
/// the user is expected to replace them with real values.
pub fn example_config_json() -> String {
    r#"{
  "hosts": [
    { "name": "main", "host": "REPLACE_WITH_HOST", "user": "REPLACE_WITH_USER" }
  ],
  "agents": [
    { "id": "hermes", "label": "Hermes Agent", "cmd": "REPLACE_WITH_HERMES_CMD" },
    { "id": "harness", "label": "DeepSeek Harness", "cmd": "REPLACE_WITH_HARNESS_CMD" }
  ]
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "asb-discovery-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const GOOD: &str = r#"{
      "hosts": [{ "name": "main", "host": "10.0.0.1", "user": "ubuntu" }],
      "agents": [{ "id": "hermes", "label": "Hermes", "cmd": "hermes chat" }]
    }"#;

    #[test]
    fn missing_files_report_searched_paths() {
        let dir = temp_dir("missing");
        let a = dir.join("a.json");
        let b = dir.join("b.json");
        let outcome = load_from_candidates(&[a.clone(), b.clone()]);
        assert_eq!(
            outcome,
            LoadOutcome::NotFound {
                searched: vec![a, b],
            }
        );
    }

    #[test]
    fn first_existing_file_wins() {
        let dir = temp_dir("first-wins");
        let a = dir.join("a.json");
        let b = dir.join("b.json");
        std::fs::write(&a, GOOD).unwrap();
        std::fs::write(
            &b,
            r#"{"hosts":[{"name":"other","host":"h"}],"agents":[{"id":"x","label":"X","cmd":"x"}]}"#,
        )
        .unwrap();
        match load_from_candidates(&[a.clone(), b]) {
            LoadOutcome::Loaded { path, config } => {
                assert_eq!(path, a);
                assert_eq!(config.hosts[0].name, "main");
            }
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn skips_missing_and_uses_next() {
        let dir = temp_dir("skip-missing");
        let missing = dir.join("nope.json");
        let present = dir.join("present.json");
        std::fs::write(&present, GOOD).unwrap();
        match load_from_candidates(&[missing, present.clone()]) {
            LoadOutcome::Loaded { path, .. } => assert_eq!(path, present),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn invalid_json_reports_path_and_error() {
        let dir = temp_dir("invalid-json");
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{ \"hosts\": [] }").unwrap();
        match load_from_candidates(&[broken.clone()]) {
            LoadOutcome::Invalid { path, errors } => {
                assert_eq!(path, broken);
                assert!(errors[0].contains("agents"), "unexpected: {errors:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn broken_first_file_is_not_silently_skipped() {
        let dir = temp_dir("no-skip-broken");
        let broken = dir.join("broken.json");
        let valid = dir.join("valid.json");
        std::fs::write(&broken, "{ not json").unwrap();
        std::fs::write(&valid, GOOD).unwrap();
        match load_from_candidates(&[broken.clone(), valid]) {
            LoadOutcome::Invalid { path, .. } => assert_eq!(path, broken),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn validation_errors_are_reported() {
        let dir = temp_dir("invalid-empty");
        let empty = dir.join("empty.json");
        std::fs::write(&empty, r#"{"hosts":[],"agents":[]}"#).unwrap();
        match load_from_candidates(&[empty]) {
            LoadOutcome::Invalid { errors, .. } => {
                assert!(errors.contains(&"hosts: must not be empty".to_string()));
                assert!(errors.contains(&"agents: must not be empty".to_string()));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn example_template_parses_and_validates() {
        let cfg = parse_config(&example_config_json()).unwrap();
        assert!(validate(&cfg).is_ok());
        assert_eq!(cfg.hosts[0].name, "main");
    }

    #[test]
    fn unique_paths_dedupes_preserving_order() {
        let a = PathBuf::from("/x/config.json");
        let b = PathBuf::from("/y/config.json");
        let out = unique_paths(vec![a.clone(), b.clone(), a.clone()]);
        assert_eq!(out, vec![a, b]);
    }
}
