#[derive(Debug, PartialEq, Eq)]
/// StateType represents the role of a node in a cluster.
pub enum StateType {
    StateFollower,
    StateCandidate,
    StateLeader,
    StatePreCandidate,
}

impl ToString for StateType {
    fn to_string(&self) -> String {
        match self {
            StateType::StateFollower => "StateFollower".to_string(),
            StateType::StateCandidate => "StateCandidate".to_string(),
            StateType::StateLeader => "StateLeader".to_string(),
            StateType::StatePreCandidate => "StatePreCandidate".to_string(),
        }
    }
}
