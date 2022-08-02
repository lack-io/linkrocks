use std::collections::VecDeque;

use raftpb::raftpb::{MessageType, HardState, Snapshot, Entry, Message};

use crate::{raft::{SoftState, Raft}, read_only::ReadState, storage::Storage};

/// Represents a Peer node in the cluster.
#[derive(Debug, Default)]
pub struct Peer {
    /// The ID of the peer.
    pub id: u64,
    /// If there is context associated with the peer (like connection information), it can be
    /// serialized and stored here.
    pub context: Option<Vec<u8>>,
}

/// The status of the snapshot
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SnapshotStatus {
    /// Represents that the snapshot is finished being created
    Finish,
    /// Indicates that the snapshot failed to build or is not ready.
    Failure,
}

/// Checks if certain message type should be used internally.
pub fn is_local_msg(t: MessageType) -> bool {
    matches!(
        t,
        MessageType::MsgHup
            | MessageType::MsgBeat
            | MessageType::MsgUnreachable
            | MessageType::MsgSnapStatus
            | MessageType::MsgCheckQuorum
    )
}

fn is_response_msg(t: MessageType) -> bool {
    matches!(
        t,
        MessageType::MsgAppResp
            | MessageType::MsgVoteResp
            | MessageType::MsgHeartbeatResp
            | MessageType::MsgUnreachable
            | MessageType::MsgPreVoteResp
    )
}

/// Ready encapsulates the entries and messages that are ready to read,
/// be saved to stable storage, committed or sent to other peers.
#[derive(Debug, Default, PartialEq)]
pub struct Ready {
    number: u64,

    ss: Option<SoftState>,

    hs: Option<HardState>,

    read_states: Vec<ReadState>,

    entries: Vec<Entry>,

    snapshot: Snapshot,

    is_persisted_msg: bool,

    light: LightReady,

    must_sync: bool,
}

/// ReadyRecord encapsulates some needed data from the corresponding Ready.
#[derive(Default, Debug, PartialEq)]
struct ReadyRecord {
    number: u64,
    // (index, term) of the last entry from the entries in Ready
    last_entry: Option<(u64, u64)>,
    // (index, term) of the snapshot in Ready
    snapshot: Option<(u64, u64)>,
}

/// LightReady encapsulates the commit index, committed entries and
/// message that are ready to be applied or be sent to other peers.
#[derive(Debug, Default, PartialEq)]
pub struct LightReady {
    commit_index: Option<u64>,
    committed_entries: Vec<Entry>,
    messages: Vec<Message>,
}

/// RawNode is a thread-unsafe Node.
/// The methods of this struct correspond to the methods of Node and are described
/// more fully there.
pub struct RawNode<T: Storage> {
    /// The internal raft state.
    pub raft: Raft<T>,
    prev_ss: SoftState,
    prev_hs: HardState,
    // Current max number of Record and ReadyRecord.
    max_number: u64,
    records: VecDeque<ReadyRecord>,
    // Index which the given committed entries should start from.
    commit_since_index: u64,
}