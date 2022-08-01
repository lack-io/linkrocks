use raftpb::prelude::HardState;

use crate::{node::SoftState, tracker::ProgressTracker, storage::Storage, raft::{Raft, StateType}};


/// Status contains information about this Raft peer and its view of the system.
/// The Process is only populated on the leader.
#[derive(Default)]
pub struct Status<'a> {
    /// The ID of the current node.
    pub id: u64,
    /// The hardstate of the raft, representing voted state.
    pub hs: HardState,
    /// The softstate of the raft, representing proposed state.
    pub ss: SoftState,
    // The index of the last entry to have been applied.
    pub applied: u64,
    /// The progress towards catching up and applying logs.
    pub progress: Option<&'a ProgressTracker>,
}

impl <'a> Status<'a> {
    /// Gets a copy of the current raft status.
    pub fn new<T: Storage>(raft: &'a Raft<T>) -> Status<'a> {
        let mut s = Status {
            id: raft.id,
            ..Default::default()
        };
        s.hs = raft.hard_state();
        s.ss = raft.soft_state();
        s.applied = raft.raft_log.applied;
        if s.ss.raft_state == StateType::Leader {
            s.progress = Some(raft.prs());
        }
        s
    }
}