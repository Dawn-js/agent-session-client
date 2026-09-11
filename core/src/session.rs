#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Connected,
    Retrying,
    Exited,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    Connected,
    Disconnected,
    RetryScheduled,
    GiveUp,
    AgentExited,
    UserClose,
}

pub fn transition(state: SessionState, event: SessionEvent) -> SessionState {
    use SessionEvent as E;
    use SessionState as S;

    match state {
        S::Closed => S::Closed,
        _ if event == E::UserClose => S::Closed,
        S::Exited => S::Exited,
        _ if event == E::AgentExited => S::Exited,
        _ => match event {
            E::Connected => S::Connected,
            E::Disconnected => S::Retrying,
            E::RetryScheduled => S::Retrying,
            E::GiveUp => S::Exited,
            E::UserClose | E::AgentExited => unreachable!("handled above"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::transition;
    use super::SessionEvent as E;
    use super::SessionState as S;

    #[test]
    fn connect_then_drop_then_retry_then_connect() {
        let s = S::Connecting;
        let s = transition(s, E::Connected);
        assert_eq!(s, S::Connected);
        let s = transition(s, E::Disconnected);
        assert_eq!(s, S::Retrying);
        let s = transition(s, E::RetryScheduled);
        assert_eq!(s, S::Retrying);
        let s = transition(s, E::Connected);
        assert_eq!(s, S::Connected);
    }

    #[test]
    fn agent_exit_is_terminal_until_closed() {
        assert_eq!(transition(S::Connected, E::AgentExited), S::Exited);
        assert_eq!(transition(S::Exited, E::Connected), S::Exited);
    }

    #[test]
    fn give_up_ends_in_exited() {
        assert_eq!(transition(S::Retrying, E::GiveUp), S::Exited);
    }

    #[test]
    fn user_close_is_terminal_from_any_state() {
        for s in [S::Connecting, S::Connected, S::Retrying, S::Exited] {
            assert_eq!(transition(s, E::UserClose), S::Closed);
        }
        assert_eq!(transition(S::Closed, E::Connected), S::Closed);
    }
}
