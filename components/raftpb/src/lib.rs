#![allow(clippy::field_reassign_with_default)]

mod confchange;
mod confstate;

pub mod raftpb {
    use std::borrow::BorrowMut;

    include!(concat!(env!("OUT_DIR"), "/raftpb.rs"));

    impl Snapshot {
        /// For a given snapshot, determine if it's empty or not.
        pub fn is_empty(&self) -> bool {
            if let Some(metadata) = self.metadata.as_ref() {
                if let Some(index) = metadata.index {
                    return index == 0;
                }
            }

            false
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
            data: Some(Vec::new()),
            metadata: Some(SnapshotMetadata {
                conf_state: None,
                index: Some(1),
                term: Some(1),
            }),
        };

        assert_eq!((&s).metadata.as_ref().unwrap().index, Some(1));
        assert!(!(&s).is_empty());

        s.metadata.as_mut().unwrap().index = Some(0);
        assert!(s.is_empty());
    }
}
