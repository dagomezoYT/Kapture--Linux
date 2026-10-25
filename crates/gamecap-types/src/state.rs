//! Session state machine types. Transitions are enforced in `gamecap-core`.

use std::fmt;

/// Lifecycle of a recording session. `Failed` is reachable from any active state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Probing,
    Ready,
    Starting,
    Recording,
    Reconfiguring,
    Degraded {
        reason: DegradedReason,
    },
    Stopping,
    Finalizing,
    Completed,
    Failed {
        reason: String,
        diagnostics: Diagnostics,
    },
}

impl SessionState {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Probing => "Probing",
            Self::Ready => "Ready",
            Self::Starting => "Starting",
            Self::Recording => "Recording",
            Self::Reconfiguring => "Reconfiguring",
            Self::Degraded { .. } => "Degraded",
            Self::Stopping => "Stopping",
            Self::Finalizing => "Finalizing",
            Self::Completed => "Completed",
            Self::Failed { .. } => "Failed",
        }
    }

    pub fn is_capturing(&self) -> bool {
        matches!(
            self,
            Self::Recording | Self::Reconfiguring | Self::Degraded { .. }
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. } | Self::Idle)
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Degraded { reason } => write!(f, "Degraded({reason})"),
            Self::Failed { reason, .. } => write!(f, "Failed({reason})"),
            other => f.write_str(other.name()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradedReason {
    pub component: String,
    pub detail: String,
}

impl fmt::Display for DegradedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.component, self.detail)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Diagnostics {
    pub last_frame_id: Option<u64>,
    pub last_error: Option<String>,
    pub notes: Vec<String>,
}

impl Diagnostics {
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}
