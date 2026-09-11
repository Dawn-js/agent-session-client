pub mod command;
pub mod exit;

pub const PROJECT_NAME: &str = "agent-session-client";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_matches_repo() {
        assert_eq!(PROJECT_NAME, "agent-session-client");
    }
}
