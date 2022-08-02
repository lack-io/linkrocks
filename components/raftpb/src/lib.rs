#![allow(clippy::field_reassign_with_default)]

mod confchange;
mod confstate;

pub use crate::confchange::{
    new_conf_change_single, parse_conf_change, stringify_conf_change, ConfChangeI,
};
pub use crate::confstate::conf_state_eq;

pub mod raftpb {
    include!(concat!(env!("OUT_DIR"), "/raftpb.rs"));

    impl Snapshot {
        /// For a given snapshot, determine if it's empty or not.
        pub fn is_empty(&self) -> bool {
            if let Some(metadata) = self.metadata.as_ref() {
                return metadata.index == 0;
            }

            false
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
}

pub mod prelude {
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
        let empty = HardState::empty();
        assert!(empty.is_empty());

        assert!(empty.eq(&HardState::empty()));
    }
}
