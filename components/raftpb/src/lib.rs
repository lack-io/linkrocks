#![allow(clippy::field_reassign_with_default)]

mod confchange;
mod confstate;
pub mod ext;
pub mod raftpb;

pub use crate::confchange::{
    new_conf_change_single, parse_conf_change, stringify_conf_change, ConfChangeI,
};
pub use crate::confstate::conf_state_eq;

pub mod prelude {
    pub use crate::ext;
    pub use crate::raftpb::{
        ConfChange, ConfChangeSingle, ConfChangeType, ConfChangeV2, ConfState, Entry, EntryType,
        HardState, Message, MessageType, Snapshot, SnapshotMetadata,
    };
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[test]
    fn test_snapshot_is_empty() {
        let mut s = Snapshot {
            data: Vec::new(),
            metadata: Some(SnapshotMetadata {
                conf_state: None,
                index: 1,
                term: 1,
            }),
        };

        assert_eq!(s.metadata.as_ref().unwrap().index, 1);
        assert!(!(&s).is_empty());

        s.metadata.as_mut().unwrap().index = 0;
        assert!(s.is_empty());
    }

    #[test]
    fn test_hard_state_is_empty() {
        let empty = HardState::new();
        assert!(empty.is_empty());

        assert!(empty.eq(&HardState::new()));
    }
}
