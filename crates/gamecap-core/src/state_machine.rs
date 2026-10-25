//! Explicit session state transitions. Failed is reachable from any non-terminal state.

use crate::error::CoreError;
use gamecap_types::SessionState;

#[derive(Debug, Clone)]
pub struct StateMachine {
    state: SessionState,
}

impl StateMachine {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
        }
    }

    pub fn current(&self) -> SessionState {
        self.state.clone()
    }

    pub fn is(&self, name: &str) -> bool {
        self.state.name() == name
    }

    pub fn transition(&mut self, next: SessionState) -> Result<(), CoreError> {
        if is_allowed(&self.state, &next) {
            self.state = next;
            Ok(())
        } else {
            Err(CoreError::InvalidTransition {
                from: self.state.to_string(),
                to: next.to_string(),
            })
        }
    }

    pub fn fail(&mut self, reason: impl Into<String>, diagnostics: gamecap_types::Diagnostics) {
        self.state = SessionState::Failed {
            reason: reason.into(),
            diagnostics,
        };
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

fn is_allowed(from: &SessionState, to: &SessionState) -> bool {
    use SessionState::*;
    if matches!(to, Failed { .. }) {
        return true;
    }
    matches!(
        (from, to),
        (Idle, Probing)
            | (Probing, Ready)
            | (Ready, Starting)
            | (Starting, Recording)
            | (Recording, Reconfiguring)
            | (Reconfiguring, Recording)
            | (Recording, Degraded { .. })
            | (Degraded { .. }, Recording)
            | (Degraded { .. }, Reconfiguring)
            | (Starting, Stopping)
            | (Recording, Stopping)
            | (Reconfiguring, Stopping)
            | (Degraded { .. }, Stopping)
            | (Stopping, Finalizing)
            | (Finalizing, Completed)
            | (Completed, Idle)
            | (Failed { .. }, Idle)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamecap_types::Diagnostics;

    #[test]
    fn happy_path() {
        let mut sm = StateMachine::new();
        assert!(sm.transition(SessionState::Probing).is_ok());
        assert!(sm.transition(SessionState::Ready).is_ok());
        assert!(sm.transition(SessionState::Starting).is_ok());
        assert!(sm.transition(SessionState::Recording).is_ok());
        assert!(sm.transition(SessionState::Stopping).is_ok());
        assert!(sm.transition(SessionState::Finalizing).is_ok());
        assert!(sm.transition(SessionState::Completed).is_ok());
    }

    #[test]
    fn rejects_skip() {
        let mut sm = StateMachine::new();
        assert!(sm.transition(SessionState::Recording).is_err());
    }

    #[test]
    fn failed_from_anywhere() {
        let mut sm = StateMachine::new();
        sm.fail("test", Diagnostics::default());
        assert!(matches!(sm.current(), SessionState::Failed { .. }));
    }
}
