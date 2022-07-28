
/// Status contains information about this Raft peer and its view of the system.
/// The Process is only populated on the leader.
pub struct Status {
    basic: BasicStatus
}

/// BasicStatus contains basic information about the Raft peer. It does not allocate.
pub struct BasicStatus {

}