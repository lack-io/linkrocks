use crate::prelude::{
    ConfState, Entry, HardState, Message, MessageType, Snapshot, SnapshotMetadata,
};

unsafe impl Send for Entry {}

impl Snapshot {
    /// For a given snapshot, determine if it's empty or not.
    pub fn is_empty(&self) -> bool {
        if let Some(metadata) = self.metadata.as_ref() {
            return metadata.index == 0;
        }

        true
    }

    pub fn new() -> Snapshot {
        ::std::default::Default::default()
    }

    pub fn get_data(&self) -> &[u8] {
        &self.data
    }
    pub fn clear_data(&mut self) {
        self.data.clear();
    }

    // Param is passed by value, moved
    pub fn set_data(&mut self, v: &[u8]) {
        self.data = v.to_vec();
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_data(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    // Take field
    pub fn take_data(&mut self) -> Vec<u8> {
        ::std::mem::replace(&mut self.data, Vec::new())
    }

    pub fn clear_metadata(&mut self) {
        self.metadata = Some(SnapshotMetadata::default());
    }

    pub fn has_metadata(&self) -> bool {
        self.metadata.is_some()
    }

    // Param is passed by value, moved
    pub fn set_metadata(&mut self, v: SnapshotMetadata) {
        self.metadata = Some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_metadata(&mut self) -> &mut SnapshotMetadata {
        if self.metadata.is_none() {
            self.metadata = Some(SnapshotMetadata::default());
        }
        self.metadata.as_mut().unwrap()
    }

    // Take field
    pub fn take_metadata(&mut self) -> SnapshotMetadata {
        self.metadata
            .take()
            .unwrap_or_else(|| SnapshotMetadata::default())
    }
}

impl SnapshotMetadata {
    pub fn new() -> SnapshotMetadata {
        ::std::default::Default::default()
    }

    // .eraftpb.ConfState conf_state = 1;

    pub fn clear_conf_state(&mut self) {
        self.conf_state = Some(ConfState::default());
    }

    pub fn has_conf_state(&self) -> bool {
        self.conf_state.is_some()
    }

    // Param is passed by value, moved
    pub fn set_conf_state(&mut self, v: ConfState) {
        self.conf_state = Some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_conf_state(&mut self) -> &mut ConfState {
        if self.conf_state.is_none() {
            self.conf_state = Some(ConfState::default());
        }
        self.conf_state.as_mut().unwrap()
    }

    // Take field
    pub fn take_conf_state(&mut self) -> ConfState {
        self.conf_state
            .take()
            .unwrap_or_else(|| ConfState::default())
    }

    // uint64 index = 2;

    pub fn get_index(&self) -> u64 {
        self.index
    }
    pub fn clear_index(&mut self) {
        self.index = 0;
    }

    // Param is passed by value, moved
    pub fn set_index(&mut self, v: u64) {
        self.index = v;
    }

    // uint64 term = 3;

    pub fn get_term(&self) -> u64 {
        self.term
    }
    pub fn clear_term(&mut self) {
        self.term = 0;
    }

    // Param is passed by value, moved
    pub fn set_term(&mut self, v: u64) {
        self.term = v;
    }
}

impl HardState {
    pub fn empty() -> Self {
        HardState {
            term: 0,
            vote: 0,
            commit: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.eq(&HardState::empty())
    }
}

impl Message {
    pub fn new() -> Message {
        ::std::default::Default::default()
    }

    // .eraftpb.MessageType msg_type = 1;

    pub fn get_msg_type(&self) -> MessageType {
        self.msg_type()
    }

    pub fn clear_msg_type(&mut self) {
        self.set_msg_type(MessageType::MsgHup)
    }

    // uint64 to = 2;

    pub fn get_to(&self) -> u64 {
        self.to
    }
    pub fn clear_to(&mut self) {
        self.to = 0;
    }

    // Param is passed by value, moved
    pub fn set_to(&mut self, v: u64) {
        self.to = v;
    }

    // uint64 from = 3;

    pub fn get_from(&self) -> u64 {
        self.from
    }
    pub fn clear_from(&mut self) {
        self.from = 0;
    }

    // Param is passed by value, moved
    pub fn set_from(&mut self, v: u64) {
        self.from = v;
    }

    // uint64 term = 4;

    pub fn get_term(&self) -> u64 {
        self.term
    }
    pub fn clear_term(&mut self) {
        self.term = 0;
    }

    // Param is passed by value, moved
    pub fn set_term(&mut self, v: u64) {
        self.term = v;
    }

    // uint64 log_term = 5;

    pub fn get_log_term(&self) -> u64 {
        self.log_term
    }
    pub fn clear_log_term(&mut self) {
        self.log_term = 0;
    }

    // Param is passed by value, moved
    pub fn set_log_term(&mut self, v: u64) {
        self.log_term = v;
    }

    // uint64 index = 6;

    pub fn get_index(&self) -> u64 {
        self.index
    }
    pub fn clear_index(&mut self) {
        self.index = 0;
    }

    // Param is passed by value, moved
    pub fn set_index(&mut self, v: u64) {
        self.index = v;
    }

    // repeated .eraftpb.Entry entries = 7;

    pub fn get_entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn clear_entries(&mut self) {
        self.entries.clear();
    }

    // Param is passed by value, moved
    pub fn set_entries(&mut self, v: Vec<Entry>) {
        self.entries = v;
    }

    // Mutable pointer to the field.
    pub fn mut_entries(&mut self) -> &mut Vec<Entry> {
        &mut self.entries
    }

    // Take field
    pub fn take_entries(&mut self) -> Vec<Entry> {
        ::std::mem::replace(&mut self.entries, Vec::new())
    }

    // uint64 commit = 8;

    pub fn get_commit(&self) -> u64 {
        self.commit
    }
    pub fn clear_commit(&mut self) {
        self.commit = 0;
    }

    // Param is passed by value, moved
    pub fn set_commit(&mut self, v: u64) {
        self.commit = v;
    }

    // uint64 commit_term = 15;

    pub fn get_commit_term(&self) -> u64 {
        self.commit_term
    }
    pub fn clear_commit_term(&mut self) {
        self.commit_term = 0;
    }

    // Param is passed by value, moved
    pub fn set_commit_term(&mut self, v: u64) {
        self.commit_term = v;
    }

    // .eraftpb.Snapshot snapshot = 9;
    pub fn clear_snapshot(&mut self) {
        self.snapshot = Some(Snapshot::default());
    }

    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    // Param is passed by value, moved
    pub fn set_snapshot(&mut self, v: Snapshot) {
        self.snapshot = Some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_snapshot(&mut self) -> &mut Snapshot {
        if self.snapshot.is_none() {
            self.snapshot = Some(Snapshot::default());
        }
        self.snapshot.as_mut().unwrap()
    }

    // Take field
    pub fn take_snapshot(&mut self) -> Snapshot {
        self.snapshot.take().unwrap_or_else(|| Snapshot::new())
    }

    // uint64 request_snapshot = 13;

    pub fn get_request_snapshot(&self) -> u64 {
        self.request_snapshot
    }
    pub fn clear_request_snapshot(&mut self) {
        self.request_snapshot = 0;
    }

    // Param is passed by value, moved
    pub fn set_request_snapshot(&mut self, v: u64) {
        self.request_snapshot = v;
    }

    // bool reject = 10;

    pub fn get_reject(&self) -> bool {
        self.reject
    }
    pub fn clear_reject(&mut self) {
        self.reject = false;
    }

    // Param is passed by value, moved
    pub fn set_reject(&mut self, v: bool) {
        self.reject = v;
    }

    // uint64 reject_hint = 11;

    pub fn get_reject_hint(&self) -> u64 {
        self.reject_hint
    }
    pub fn clear_reject_hint(&mut self) {
        self.reject_hint = 0;
    }

    // Param is passed by value, moved
    pub fn set_reject_hint(&mut self, v: u64) {
        self.reject_hint = v;
    }

    // bytes context = 12;

    pub fn get_context(&self) -> &[u8] {
        &self.context
    }
    pub fn clear_context(&mut self) {
        self.context.clear();
    }

    // Param is passed by value, moved
    pub fn set_context(&mut self, v: Vec<u8>) {
        self.context = v;
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_context(&mut self) -> &mut Vec<u8> {
        &mut self.context
    }

    // Take field
    pub fn take_context(&mut self) -> Vec<u8> {
        ::std::mem::replace(&mut self.context, Vec::new())
    }

    // uint64 priority = 14;

    pub fn get_priority(&self) -> u64 {
        self.priority
    }
    pub fn clear_priority(&mut self) {
        self.priority = 0;
    }

    // Param is passed by value, moved
    pub fn set_priority(&mut self, v: u64) {
        self.priority = v;
    }
}

impl<Iter1, Iter2> From<(Iter1, Iter2)> for ConfState
where
    Iter1: IntoIterator<Item = u64>,
    Iter2: IntoIterator<Item = u64>,
{
    fn from((voters, learners): (Iter1, Iter2)) -> Self {
        let mut conf_state = ConfState::default();
        conf_state.voters.extend(voters.into_iter());
        conf_state.learners.extend(learners.into_iter());
        conf_state
    }
}
