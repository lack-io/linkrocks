use std::fmt::Display;

/// The state of the progress.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProgressState {
    /// Whether it's probing.
    Probe,
    /// Whether it's replicating.
    Replicate,
    /// Whether it's a snapshot
    Snapshot,
}

impl Default for ProgressState {
    fn default() -> Self {
        ProgressState::Probe
    }
}

impl Display for ProgressState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressState::Probe => write!(f, "StateProbe"),
            ProgressState::Replicate => write!(f, "StateReplicate"),
            ProgressState::Snapshot => write!(f, "StateSnapshot"),
        }
    }
}
