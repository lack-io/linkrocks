use crate::{errors::Result, raft::StateType, read_only::ReadState, status::Status};

use raftpb::{
    prelude::{Entry, HardState, Message, Snapshot},
    ConfChangeI,
};
use tokio_context::context::Context;

use async_trait::async_trait;
use tokio::sync::broadcast::Receiver;

pub enum SnapshotStatus {
    SnapshotFinish,
    SnapshotFailure,
}

#[derive(Debug, Default)]
/// SoftState provides state that is useful for logging and debugging.
/// The state is volatile and does not need to be persisted to the WAL.
pub struct SoftState {
    // must use atomic operations to access; keep 64-bit aligned.
    pub leader_id: u64,
    pub raft_state: StateType,
}

impl PartialEq for SoftState {
    fn eq(&self, other: &Self) -> bool {
        self.leader_id == other.leader_id && self.raft_state == other.raft_state
    }
}

/// Ready encapsulates the entries and messages that are ready to read,
/// be saved to stable storage, committed or sent to other peers.
/// All fields in Ready are read-only.
pub struct Ready {
    /// The current volatile state of a Node.
    /// SoftState will be None if there is no update.
    /// It is not required to consume or store SoftState.
    soft_state: Option<SoftState>,

    /// The current state of a Node to be saved to stable storage BEFORE
    /// Messages are sent.
    /// HardState will be equal to empty state if there is no update.
    hard_state: HardState,

    /// read_states can be used for node to serve linearizable read requests locally
    /// when its applied index is greater than the index in ReadState.
    /// Note that the readState will be returned when raft receives msgReadIndex.
    /// The returned is only valid for the request that requested to read.
    read_states: Vec<ReadState>,

    /// entries specifies entries to be saved to stable storage BEFORE
    /// Messages are sent.
    entries: Vec<Entry>,

    /// snapshot specifies the snapshot to be saved to stable storage.
    snapshot: Snapshot,

    /// committed_entries specifies entries to be committed to a
    /// store/state-machine. These have previously been committed to stable
    /// store.
    committed_entries: Vec<Entry>,

    /// messages specifies outbound messages to be sent AFTER Entries are
    /// committed to stable storage.
    /// If it contains a MsgSnap message, the application MUST report back to raft
    /// when the snapshot has been received or has failed by calling ReportSnapshot.
    messages: Vec<Message>,

    /// must_sync indicates whether the HardState and Entries must be synchronously
    /// written to disk or if an asynchronous write is permissible.
    must_sync: bool,
}

impl Ready {
    fn contains_updates(&self) -> bool {
        self.soft_state.is_none()
            || !self.hard_state.is_empty()
            || !self.snapshot.is_empty()
            || self.entries.len() > 0
            || self.committed_entries.len() > 0
            || self.messages.len() > 0
            || self.read_states.len() != 0
    }

    // applied_cursor extracts from the Ready the highest index the client has
    // applied (once the Ready is confirmed via advance). If no information is
    // contained in the Ready, returns zero.
    fn applied_cursor(&self) -> u64 {
        let n = self.committed_entries.len();
        if n > 0 {
            return self.committed_entries[n - 1].index.unwrap();
        }
        if let Some(ref metadata) = self.snapshot.metadata {
            if let Some(index) = metadata.index {
                if index > 0 {
                    return index;
                }
            }
        }
        0
    }
}

/// Node represents a node in raft cluster.
#[async_trait]
pub trait Node {
    /// tick increments the internal logical clock for the Node by a single tick. Election
    /// timeouts and heartbeat timeouts are in units of ticks.
    async fn tick(&self);

    /// campaign causes the Node to transition to candidate state and start campaigning to become leader.
    async fn campaign(&self, &mut ctx: Context) -> Result<()>;

    /// proposes that data be appended to the log. Note that proposals can be lost without
    /// notice, therefore it is user's job to ensure proposal retries.
    async fn propose(&self, &mut ctx: Context, data: &[u8]) -> Result<()>;

    /// proposes a configuration change. Like any proposal, the
    /// configuration change may be dropped with or without an error being
    /// retruned. In particular, configuration changes are dropped unless the
    /// leader has certainty that there is no prior unapplied configuration
    /// change in its log.
    async fn proposal_conf_change(&self, &mut ctx: Context) -> Result<()>;

    /// advances the state machine using the given message.
    async fn step(&self, &mut ctx: Context) -> Result<()>;

    /// returns a channel that returns the current point-in-time state.
    /// Users of the Node must call advance after retrieving the state retruned by Ready.
    ///
    /// Note: No committe entries from the next Ready may be applied until all committed entries
    /// and snapshots from the previous one have finished.
    async fn ready(&self) -> Receiver<Ready>;

    /// notifies the Node that the application has saved progress up to the last Ready.
    /// It prepares the node to return the next available Ready.
    ///
    /// The application should generally call advance after applies the entries in last Ready.
    ///
    /// However, as an optimization, the application may call `advance` while it is applying the
    /// commands. For example, when the last Ready contains a snapshot, the application might take
    /// a long time to apply the snapshot data. To continue receiving Ready without blocking raft
    /// progress, it can call `advance` before finishing applying the last ready.
    async fn advance(&self);

    /// applies a config change (previously passed to `propose_conf_change`) to the node. This muse be
    /// called whenever a config change is observed in `Ready.committed_entries`, expcept when the app
    /// decides to reject the configuration change (i.e. treats it as a noop instead), in which case it
    /// must bot be called.
    async fn apply_conf_change(&self, cc: Box<dyn ConfChangeI>);

    /// attempts to transfer leadership to be given transferee.
    async fn transfer_leadership(&self, lead: u64, transfer: u64);

    /// requests a read state. The read state will be set in the ready.
    /// Read state has a read index. Once the application advances further than the read
    /// index, any linerizable read requests issued before the read request can be
    /// processed safely. The read state will have the same rctx attached.
    /// Note that request can be lost without notice, therefore it is user's job
    /// to ensure read index retries.
    async fn read_index(&self, mut rctx: &[u8]) -> Result<()>;

    /// retruns the current status of the raft state machine
    async fn status(&self) -> Status;

    /// reports the given node is not reachable for the last send.
    async fn report_unreachable(&self, id: u64);

    /// reports the status of the sent snapshot. The id is the raft ID of the follower
    /// who is meant to receive the snapshot, and the status is SnapshotFinish or SnapshotFailure.
    /// Calling ReportSnapshot with SnapshotFinish is a no-op. But, any failure in applying a
    /// snapshot (for e.g., while streaming it from leader to follower), should be reported to the
    /// leader with SnapshotFailure. When leader sends a snapshot to a follower, it pauses any raft
    /// log probes until the follower can apply the snapshot and advance its state. If the follower
    /// can't do that, for e.g., due to a crash, it could end up in a limbo, never getting any
    /// updates from the leader. Therefore, it is crucial that the application ensures that any
    /// failure in snapshot sending is caught and reported back to the leader; so it can resume raft
    /// log probing in the follower.
    async fn report_snapshot(&self, id: u64, status: SnapshotStatus);

    /// Stop performs any necessary termination of the Node.
    async fn stop(&self);
}

#[derive(Debug, Clone)]
pub struct Peer {
    id: u64,
    context: Vec<u8>,
}

pub struct node {

}
