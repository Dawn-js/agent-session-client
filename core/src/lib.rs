pub mod backoff;
pub mod command;
pub mod config;
pub mod discovery;
pub mod exit;
pub mod keys;
pub mod reconnect;
pub mod session;
pub mod transport;

pub const PROJECT_NAME: &str = "agent-session-client";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_matches_repo() {
        assert_eq!(PROJECT_NAME, "agent-session-client");
    }
}
