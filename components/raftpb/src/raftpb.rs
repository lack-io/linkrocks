#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Entry {
    /// must be 64-bit aligned for atomic operations
    #[prost(uint64, tag="2")]
    pub term: u64,
    /// must be 64-bit aligned for atomic operations
    #[prost(uint64, tag="3")]
    pub index: u64,
    #[prost(enumeration="EntryType", tag="1")]
    pub entry_type: i32,
    #[prost(bytes="vec", tag="4")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes="vec", tag="5")]
    pub context: ::prost::alloc::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SnapshotMetadata {
    #[prost(message, optional, tag="1")]
    pub conf_state: ::core::option::Option<ConfState>,
    #[prost(uint64, tag="2")]
    pub index: u64,
    #[prost(uint64, tag="3")]
    pub term: u64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Snapshot {
    #[prost(bytes="vec", tag="1")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    #[prost(message, optional, tag="2")]
    pub metadata: ::core::option::Option<SnapshotMetadata>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Message {
    #[prost(enumeration="MessageType", tag="1")]
    pub msg_type: i32,
    #[prost(uint64, tag="2")]
    pub to: u64,
    #[prost(uint64, tag="3")]
    pub from: u64,
    #[prost(uint64, tag="4")]
    pub term: u64,
    /// logTerm is generally used for appending Raft logs to followers. For example,
    /// (type=MsgApp,index=100,logTerm=5) means leader appends entries starting at
    /// index=101, and the term of entry at index 100 is 5.
    /// (type=MsgAppResp,reject=true,index=100,logTerm=5) means follower rejects some
    /// entries from its leader as it already has an entry with term 5 at index 100.
    #[prost(uint64, tag="5")]
    pub log_term: u64,
    #[prost(uint64, tag="6")]
    pub index: u64,
    #[prost(message, repeated, tag="7")]
    pub entries: ::prost::alloc::vec::Vec<Entry>,
    #[prost(uint64, tag="8")]
    pub commit: u64,
    #[prost(uint64, tag="9")]
    pub commit_term: u64,
    #[prost(message, optional, tag="10")]
    pub snapshot: ::core::option::Option<Snapshot>,
    #[prost(bool, tag="11")]
    pub reject: bool,
    #[prost(uint64, tag="12")]
    pub reject_hint: u64,
    #[prost(bytes="vec", tag="13")]
    pub context: ::prost::alloc::vec::Vec<u8>,
    #[prost(uint64, tag="14")]
    pub request_snapshot: u64,
    #[prost(uint64, tag="15")]
    pub priority: u64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HardState {
    #[prost(uint64, tag="1")]
    pub term: u64,
    #[prost(uint64, tag="2")]
    pub vote: u64,
    #[prost(uint64, tag="3")]
    pub commit: u64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfState {
    /// The voters in the incoming config. (If the configuration is not joint,
    /// then the outgoing config is empty).
    #[prost(uint64, repeated, tag="1")]
    pub voters: ::prost::alloc::vec::Vec<u64>,
    /// The learners in the incoming config.
    #[prost(uint64, repeated, tag="2")]
    pub learners: ::prost::alloc::vec::Vec<u64>,
    /// The voters in the outgoing config.
    #[prost(uint64, repeated, tag="3")]
    pub voters_outgoing: ::prost::alloc::vec::Vec<u64>,
    /// The nodes that will become learners when the outgoing config is removed.
    /// These nodes are necessarily currently in nodes_joint (or they would have
    /// been added to the incoming config right away).
    #[prost(uint64, repeated, tag="4")]
    pub learners_next: ::prost::alloc::vec::Vec<u64>,
    /// If set, the config is joint and Raft will automatically transition into
    /// the final config (i.e. remove the outgoing config) when this is safe.
    #[prost(bool, tag="5")]
    pub auto_leave: bool,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfChange {
    #[prost(enumeration="ConfChangeType", tag="2")]
    pub change_type: i32,
    #[prost(uint64, tag="3")]
    pub node_id: u64,
    #[prost(bytes="vec", tag="4")]
    pub context: ::prost::alloc::vec::Vec<u8>,
    /// NB: this is used only by etcd to thread through a unique identifier.
    /// Ideally it should really use the Context instead. No counterpart to
    /// this field exists in ConfChangeV2.
    #[prost(uint64, tag="1")]
    pub id: u64,
}
/// ConfChangeSingle is an individual configuration change operation. Multiple
/// such operations can be carried out atomically via a ConfChangeV2.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfChangeSingle {
    #[prost(enumeration="ConfChangeType", tag="1")]
    pub change_type: i32,
    #[prost(uint64, tag="2")]
    pub node_id: u64,
}
/// ConfChangeV2 messages initiate configuration changes. They support both the
/// simple "one at a time" membership change protocol and full Joint Consensus
/// allowing for arbitrary changes in membership.
///
/// The supplied context is treated as an opaque payload and can be used to
/// attach an action on the state machine to the application of the config change
/// proposal. Note that contrary to Joint Consensus as outlined in the Raft
/// paper\[1\], configuration changes become active when they are *applied* to the
/// state machine (not when they are appended to the log).
///
/// The simple protocol can be used whenever only a single change is made.
///
/// Non-simple changes require the use of Joint Consensus, for which two
/// configuration changes are run. The first configuration change specifies the
/// desired changes and transitions the Raft group into the joint configuration,
/// in which quorum requires a majority of both the pre-changes and post-changes
/// configuration. Joint Consensus avoids entering fragile intermediate
/// configurations that could compromise survivability. For example, without the
/// use of Joint Consensus and running across three availability zones with a
/// replication factor of three, it is not possible to replace a voter without
/// entering an intermediate configuration that does not survive the outage of
/// one availability zone.
///
/// The provided ConfChangeTransition specifies how (and whether) Joint Consensus
/// is used, and assigns the task of leaving the joint configuration either to
/// Raft or the application. Leaving the joint configuration is accomplished by
/// proposing a ConfChangeV2 with only and optionally the Context field
/// populated.
///
/// For details on Raft membership changes, see:
///
/// \[1\]: <https://github.com/ongardie/dissertation/blob/master/online-trim.pdf>
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfChangeV2 {
    #[prost(enumeration="ConfChangeTransition", tag="1")]
    pub transition: i32,
    #[prost(message, repeated, tag="2")]
    pub changes: ::prost::alloc::vec::Vec<ConfChangeSingle>,
    #[prost(bytes="vec", tag="3")]
    pub context: ::prost::alloc::vec::Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum EntryType {
    EntryNormal = 0,
    /// corresponds to pb.ConfChange
    EntryConfChange = 1,
    /// corresponds to pb.ConfChangeV2
    EntryConfChangeV2 = 2,
}
/// For description of different message types, see:
/// <https://pkg.go.dev/go.etcd.io/etcd/raft/v3#hdr-MessageType>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum MessageType {
    MsgHup = 0,
    MsgBeat = 1,
    MsgProp = 2,
    MsgApp = 3,
    MsgAppResp = 4,
    MsgVote = 5,
    MsgVoteResp = 6,
    MsgSnap = 7,
    MsgHeartbeat = 8,
    MsgHeartbeatResp = 9,
    MsgUnreachable = 10,
    MsgSnapStatus = 11,
    MsgCheckQuorum = 12,
    MsgTransferLeader = 13,
    MsgTimeoutNow = 14,
    MsgReadIndex = 15,
    MsgReadIndexResp = 16,
    MsgPreVote = 17,
    MsgPreVoteResp = 18,
}
/// ConfChangeTransition specifies the behavior of a configuration change with
/// respect to joint consensus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum ConfChangeTransition {
    /// Automatically use the simple protocol if possible, otherwise fall back
    /// to ConfChangeJointImplicit. Most applications will want to use this.
    Auto = 0,
    /// Use joint consensus unconditionally, and transition out of them
    /// automatically (by proposing a zero configuration change).
    ///
    /// This option is suitable for applications that want to minimize the time
    /// spent in the joint configuration and do not store the joint configuration
    /// in the state machine (outside of InitialState).
    JointImplicit = 1,
    /// Use joint consensus and remain in the joint configuration until the
    /// application proposes a no-op configuration change. This is suitable for
    /// applications that want to explicitly control the transitions, for example
    /// to use a custom payload (via the Context field).
    JointExplicit = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum ConfChangeType {
    ConfChangeAddNode = 0,
    ConfChangeRemoveNode = 1,
    ConfChangeUpdateNode = 2,
    ConfChangeAddLearnerNode = 3,
}
