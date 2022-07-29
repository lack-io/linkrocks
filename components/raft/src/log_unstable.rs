use crate::prelude::{Entry, Snapshot};
use slog::{crit, info, Logger};

// unstable.entries[i] has raft log position i+unstable.offset.
// Note that unstable.offset may be less than the highest log
// position in storage; this means that the next write to storage
// might need to truncate the log before persisting unstable.entries.
pub struct unstable {
    // the incoming unstable snapshot, if any.
    snapshot: Option<Snapshot>,
    // all entries that have not yet been written to storage.
    entries: Vec<Entry>,
    offset: u64,

    logger: Logger,
}

impl unstable {
    /// returns the index of the first possible entry in entries
    /// if it has a snapshot.
    fn maybe_first_index(&self) -> Option<u64> {
        let metadata = self.snapshot.as_ref()?.metadata.as_ref()?;
        Some(metadata.index() + 1)
    }

    /// returns the last index if it has at least one
    /// unstable entry or snapshot.
    fn maybe_last_index(&self) -> Option<u64> {
        match self.entries.len() {
            0 => {
                let metadata = self.snapshot.as_ref()?.metadata.as_ref()?;
                Some(metadata.index())
            }
            l => Some(self.offset + l as u64),
        }
    }

    /// maybe_term returns the term of the entry at index i, if there
    /// is any.
    fn maybe_term(&self, i: u64) -> Option<u64> {
        if i < self.offset {
            let snapshot = self.snapshot.as_ref()?;
            let metadata = snapshot.metadata.as_ref()?;
            if metadata.index() == i {
                return Some(metadata.term());
            }
            return None;
        }

        let last = self.maybe_last_index()?;
        if i > last {
            return None;
        };
        Some(self.entries[(i - self.offset) as usize].term())
    }

    fn stable_to(&mut self, i: u64, t: u64) {
        let gt = self.maybe_term(i);
        if gt.is_none() {
            return;
        }
        // if i < offset, term is matched with the snapshot
        // only update the unstable entries if term is matched with
        // an unstable entry.
        if gt.unwrap() == t && i >= self.offset {
            self.entries = self.entries[(i + 1 - self.offset) as usize..].to_vec();
            self.offset = i + 1;
            self.shrink_entries_array();
        }
    }

    // shrink_entries_array discards the underlying array used by the entries slice
    // if most of it isn't being used. This avoids holding references to a bunch of
    // potentially large entries that aren't needed anymore. Simply clearing the
    // entries wouldn't be safe because clients might still be using them.
    fn shrink_entries_array(&mut self) {
        // We replace the array if we're using less than half of the space in
        // it. This number is fairly arbitrary, chosen as an attempt to balance
        // memory usage vs number of allocations. It could probably be improved
        // with some focused tuning.
        let len_multiple = 2;
        if self.entries.len() == 0 {
            self.entries.clear();
        } else if self.entries.len() * len_multiple < self.entries.capacity() {
            let new_entries = self.entries.clone();
            self.entries = new_entries;
        }
    }

    fn stable_snap_to(&mut self, i: u64) {
        if let Some(snapshot) = &self.snapshot {
            if let Some(metadata) = &snapshot.metadata {
                if metadata.index() == i {
                    self.snapshot = None;
                }
            }
        }
    }

    fn restore(&mut self, s: Snapshot) {
        // u.offset = s.Metadata.Index + 1
        if let Some(m) = s.metadata.as_ref() {
            self.offset = m.index() + 1;
        }
        self.entries.clear();
        self.snapshot = Some(s);
    }

    fn truncate_and_append(&mut self, ents: Vec<Entry>) {
        let after = ents[0].index();
        if after == self.offset + self.entries.len() as u64 {
            // after is the next index in the entries
            // directly append
            self.entries.extend(ents);
        } else if after <= self.offset {
            info!(
                self.logger,
                "replace the unstable entries from index {}", after
            );
            // The log is being truncated to before our current offset
            // portion, so set the offset and replace the entries
            self.offset = after;
            self.entries = ents;
        } else {
            // truncate to after and copy to u.entries
            // then append
            info!(
                self.logger,
                "truncate the unstable entries before index {}", after
            );
            self.entries = self.slice(self.offset, after);
            self.entries.extend(ents);
        }
    }

    fn slice(&self, low: u64, high: u64) -> Vec<Entry> {
        self.must_check_out_of_bounds(low, high);
        self.entries[(low - self.offset) as usize..(high - self.offset) as usize].to_vec()
    }

    // u.offset <= lo <= hi <= u.offset+len(u.entries)
    fn must_check_out_of_bounds(&self, low: u64, high: u64) {
        if low > high {
            crit!(self.logger, "invalid unstable.slice {} > {}", low, high);
        }
        let upper = self.offset + self.entries.len() as u64;
        if low < self.offset || high > upper {
            crit!(
                self.logger,
                "unstable.slice[{},{}) out of bound [{},{}]",
                low,
                high,
                self.offset,
                upper
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::default_logger;

    use super::unstable;
    use raftpb::prelude::*;

    #[test]
    fn test_log_unstable_maybe_first_index() {
        let mut s = Snapshot::default();
        s.metadata = Some(SnapshotMetadata::default());
        let unstable = unstable {
            snapshot: Some(s),
            entries: vec![],
            offset: 0,
            logger: default_logger(),
        };

        assert!(unstable.maybe_first_index().is_some());
    }

    #[test]
    fn test_log_unstable_maybe_last_index() {
        let mut s = Snapshot::default();
        s.metadata = Some(SnapshotMetadata::default());
        let mut unstable = unstable {
            snapshot: Some(s),
            entries: vec![],
            offset: 0,
            logger: default_logger(),
        };

        assert!(unstable.maybe_last_index().is_some());

        unstable.entries.push(Entry {
            term: Some(1),
            index: Some(1),
            r#type: Some(1),
            data: Some(vec![]),
        });
        assert_eq!(unstable.maybe_last_index(), Some(1));
        unstable.offset += 1;
        assert_eq!(unstable.maybe_last_index(), Some(2));
    }
}
