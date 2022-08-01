use std::ops::{Deref, DerefMut};

use getset::Getters;
use raftpb::prelude::{HardState, Message};

use crate::{node::SoftState, raft_log::RaftLog, storage::Storage, tracker::ProgressTracker};

/// StateType represents the role of a node in a cluster.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StateType {
    Follower,
    Candidate,
    Leader,
    PreCandidate,
}

impl Default for StateType {
    fn default() -> Self {
        StateType::Follower
    }
}

impl ToString for StateType {
    fn to_string(&self) -> String {
        match self {
            StateType::Follower => "Follower".to_string(),
            StateType::Candidate => "Candidate".to_string(),
            StateType::Leader => "Leader".to_string(),
            StateType::PreCandidate => "PreCandidate".to_string(),
        }
    }
}

/// A constant represents invalid id of raft.
pub const INVALID_ID: u64 = 0;
/// A constant represents invalid index of raft log.
pub const INVALID_INDEX: u64 = 0;

/// The core struct of raft consensus
///
/// It's a helper struct to get around rust borrow checks.
#[derive(Getters)]
pub struct RaftCore<T: Storage> {
    /// The current election term.
    pub term: u64,

    /// Which peer this raft is voting for.
    pub vote: u64,

    pub id: u64,
    /// The persistent log.
    pub raft_log: RaftLog<T>,

    /// The current role of this node.
    pub state: StateType,

    /// The leader id
    pub leader_id: u64,
}

/// A struct that represents the raft consensus itself, Stores details concerning the current
/// and possible state the system can take.
pub struct Raft<T: Storage> {
    prs: ProgressTracker,

    /// The list of messages.
    pub msg: Vec<Message>,
    /// Internal raftCore.
    pub r: RaftCore<T>,
}

impl<T: Storage> Deref for Raft<T> {
    type Target = RaftCore<T>;

    #[inline]
    fn deref(&self) -> &RaftCore<T> {
        &self.r
    }
}

impl<T: Storage> DerefMut for Raft<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.r
    }
}

impl<T: Storage> Raft<T> {
    /// Returns a value representing the softstate at the time of calling.
    pub fn soft_state(&self) -> SoftState {
        SoftState {
            leader_id: self.leader_id,
            raft_state: self.state,
        }
    }

    /// Returns a value representing the hardstate at the time of calling.
    pub fn hard_state(&self) -> HardState {
        let mut hs = HardState::default();
        hs.term = Some(self.term);
        hs.vote = Some(self.vote);
        hs.commit = Some(self.raft_log.committed);
        hs
    }

    /// Returns a read-only reference to the progress set.
    pub fn prs(&self) -> &ProgressTracker {
        &self.prs
    }
}
