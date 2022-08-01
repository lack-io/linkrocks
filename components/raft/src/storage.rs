use std::{cmp::max, sync::Arc};

use crate::prelude::{ConfState, Entry, HardState, Snapshot, SnapshotMetadata};

use async_trait::async_trait;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
    errors::{Error, Result, StorageError},
    util::limit_size,
};

use getset::{Getters, Setters};

/// Holds both the hard state (commit index, vote leader, term) and the configuration state
/// (Current node IDs)
#[derive(Debug, Clone, Default, Getters, Setters)]
pub struct RaftState {
    /// Contains the last meta information including commit index, the vote leader, and the vote term.
    pub hard_state: HardState,

    /// Records the current node IDs like `[1, 2, 3]` in the cluster. Every Raft node must have a
    /// unique ID in the cluster;
    pub conf_state: ConfState,
}

impl RaftState {
    /// Create a new RaftState.
    pub fn new(hard_state: HardState, conf_state: ConfState) -> RaftState {
        RaftState {
            hard_state,
            conf_state,
        }
    }
    /// Indicates the `RaftState` is initialized or not.
    pub fn initialized(&self) -> bool {
        self.conf_state != ConfState::default()
    }
}

/// Records the context of the caller who calls entries() of Storage trait.
#[derive(Debug, Clone)]
pub struct GetEntriesContext(pub(crate) GetEntriesFor);

impl GetEntriesContext {
    /// Used for callers out of raft. Caller can customize if it supports async.
    pub fn empty(can_async: bool) -> Self {
        GetEntriesContext(GetEntriesFor::Empty(can_async))
    }

    /// Check if the caller's context support fetching entries asynchrouously.
    pub fn can_async(&self) -> bool {
        match self.0 {
            GetEntriesFor::SendAppend { .. } => true,
            GetEntriesFor::Empty(can_async) => can_async,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum GetEntriesFor {
    // for sending entries to followers
    SendAppend {
        /// the peer id to which the entries are going to send
        to: u64,
        /// the term when the request is issued
        term: u64,
        /// whether to exhaust all the entries
        aggressively: bool,
    },
    // for getting committed entries in a ready
    GenReady,
    // for getting entries to check pending conf when transferring leader
    TransferLeader,
    // for getting entries to check pending conf when forwarding commit index by vote messages
    CommitByVote,
    // It's not called by the raft itself
    Empty(bool),
}

/// Storage is an interface that may be implemented by the application
/// to retrieve log entries from storage.
///
/// If any Storage method returns error, the raft instance will
/// become inoperable and refuse to participate in elections; the
/// application is responsible for cleanup and recovery in this case.
#[async_trait]
pub trait Storage {
    /// returns the saved HardState and ConfState information
    async fn inital_state(&self) -> Result<RaftState>;

    /// `entries` returns a slice of log entries in the range [low,high].
    /// MaxSize limits the total size of the log entries returned, but
    /// Entries returns at least one entry if any.
    async fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: Option<u64>,
        context: GetEntriesContext,
    ) -> Result<Vec<Entry>>;

    /// `term` returns the term of entry i, which must be in the range
    /// [first_index()-1, last_index()]. The term of the entry before
    /// first_index is retained for matching purposes even though the
    /// rest of that entry amy not be avaiable.
    async fn term(&self, i: u64) -> Result<u64>;

    /// `last_index` retruns the index of the last entry in the log.
    async fn last_index(&self) -> Result<u64>;

    /// `first_index` returns the index of the first log entry that is
    /// possibly available via `entries` (older entries have been incorporated)
    /// into the latest `snapshot`; if storage only contains the dummy entry the
    /// first log entry is not available).
    async fn first_index(&self) -> Result<u64>;

    /// `snapshot` returns the most recent snapshot.
    /// If snapshot is temporarily unavailable, it should return StorageError::SnapshotTemporarilyUnavailable
    /// snapshot and call `snapshot` later.
    async fn snapshot(&self, request_index: u64, to: u64) -> Result<Snapshot>;
}

/// MemStorage implements the `Storage` trait backed by an in-memory array.
#[derive(Debug, Clone, Default)]
pub struct MemStorageCore {
    raft_state: RaftState,
    snapshot_metadata: SnapshotMetadata,
    /// ents[i] has raft log position i+snapshot.metadata.index()
    ents: Vec<Entry>,

    // If it is true, the next snapshot will return a
    // SnapshotTemporarilyUnavailable error.
    trigger_snap_unavailable: bool,
    // Peers that are fetching entries asynchronously.
    trigger_log_unavailable: bool,
    // Stores get entries context.
    get_entries_context: Option<GetEntriesContext>,
}

impl MemStorageCore {
    fn new() -> Self {
        MemStorageCore {
            ..Default::default()
        }
    }

    /// Saves the current HardState
    pub fn set_hardstate(&mut self, hs: HardState) {
        self.raft_state.hard_state = hs;
    }

    /// Get the hard state.
    pub fn hard_state(&self) -> &HardState {
        &self.raft_state.hard_state
    }

    /// Get the mut hard state.
    pub fn mut_hard_state(&mut self) -> &mut HardState {
        &mut self.raft_state.hard_state
    }

    /// Saves the current conf state
    pub fn set_conf_state(&mut self, cs: ConfState) {
        self.raft_state.conf_state = cs;
    }

    pub fn entries(
        &mut self,
        low: u64,
        high: u64,
        max_size: Option<u64>,
        context: GetEntriesContext,
    ) -> Result<Vec<Entry>> {
        if low < self.first_index() {
            return Err(Error::Store(StorageError::Compacted));
        }
        if high > self.last_index() + 1 {
            panic!(
                "index out of bound (last: {}, high: {})",
                self.last_index() + 1,
                high,
            )
        }
        if self.trigger_log_unavailable && context.can_async() {
            self.get_entries_context = Some(context);
            return Err(Error::Store(StorageError::LogTemporarilyUnavailable));
        }

        let offset = self.ents[0].index();
        let mut entries: Vec<Entry> =
            self.ents[(low - offset) as usize..(high - offset) as usize].to_vec();
        limit_size(&mut entries, max_size);
        Ok(entries)
    }

    pub fn term(&self, i: u64) -> Result<u64> {
        let offset = self.first_index();
        if i < offset {
            return Err(Error::Store(StorageError::Compacted));
        }
        let at = (i - offset) as usize;
        if at >= self.ents.len() {
            return Err(Error::Store(StorageError::Unavailable));
        }
        Ok(self.ents[at].term())
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = Snapshot::default();

        let mut meta = SnapshotMetadata::default();
        meta.index = self.raft_state.hard_state.commit;
        meta.term = match meta.index().cmp(&self.snapshot_metadata.index()) {
            std::cmp::Ordering::Equal => self.snapshot_metadata.term,
            std::cmp::Ordering::Greater => {
                let offset = self.ents[0].index();
                self.ents[(meta.index() - offset) as usize].term
            }
            std::cmp::Ordering::Less => {
                panic!(
                    "commit {} < snapshot_metadata.index {}",
                    meta.index(),
                    self.snapshot_metadata.index()
                )
            }
        };

        meta.conf_state = Some(self.raft_state.conf_state.clone());
        snapshot.metadata = Some(meta);
        snapshot
    }

    #[inline]
    fn has_entry_at(&self, index: u64) -> bool {
        !self.ents.is_empty() && index >= self.first_index() && index <= self.last_index()
    }

    fn first_index(&self) -> u64 {
        match self.ents.first() {
            Some(e) => e.index(),
            None => self.snapshot_metadata.index() + 1,
        }
    }

    fn last_index(&self) -> u64 {
        match self.ents.last() {
            Some(e) => e.index(),
            None => self.snapshot_metadata.index(),
        }
    }

    // Overwrites the contents of this Storage object with those of the given snapshot.
    ///
    /// # Panics
    ///
    /// Panics if the snapshot index is less than the storage's first index.
    pub fn apply_snapshot(&mut self, snap: Snapshot) -> Result<()> {
        let meta = snap.metadata.unwrap();
        let index = meta.index();

        if self.first_index() > index {
            return Err(Error::Store(StorageError::SnapshotOutOfDate));
        }

        self.snapshot_metadata = meta.clone();

        self.raft_state.hard_state.term = Some(max(self.raft_state.hard_state.term(), meta.term()));
        self.raft_state.hard_state.commit = Some(index);
        self.ents.clear();

        // Update conf states.
        self.raft_state.conf_state = meta.conf_state.unwrap_or_else(|| ConfState::default());
        Ok(())
    }

    /// discards all log entries prior to compact_index.
    /// It is the application's responsibility to not attempt to compact an index
    /// greater than raftLog.applied.
    pub fn compact(&mut self, compact_index: u64) -> Result<()> {
        if compact_index <= self.first_index() {
            // Don't need to treat this case as an error.
            return Ok(());
        }

        if compact_index > self.last_index() + 1 {
            panic!(
                "compact not received raft logs: {}, last index: {}",
                compact_index,
                self.last_index()
            );
        }

        if let Some(entry) = self.ents.first() {
            let offset = compact_index - entry.index();
            self.ents.drain(..offset as usize);
        }

        Ok(())
    }

    /// Append the new entries to storage.
    ///
    /// # Panics
    ///
    /// Panics if `ents` contains compacted entries, or there's a gap between `ents` and the last
    /// received entry in the storage.
    pub fn append(&mut self, ents: &[Entry]) -> Result<()> {
        if ents.is_empty() {
            return Ok(());
        }
        if self.first_index() > ents[0].index() {
            panic!(
                "overwrite compacted raft logs, compacted: {}, append: {}",
                self.first_index() - 1,
                ents[0].index(),
            );
        }
        if self.last_index() + 1 < ents[0].index() {
            panic!(
                "raft logs should be continuous, last index: {}, new appended: {}",
                self.last_index(),
                ents[0].index()
            );
        }

        // Remove all entries overwritten by `ents`.
        let diff = ents[0].index() - self.first_index();
        self.ents.drain(diff as usize..);
        self.ents.extend_from_slice(ents);

        Ok(())
    }

    /// Trigger a SnapshotTemporarilyUnavailable error.
    pub fn trigger_snap_unavailable(&mut self) {
        self.trigger_snap_unavailable = true;
    }

    /// Set a LogTemporarilyUnavailable error.
    pub fn trigger_log_unavailable(&mut self, v: bool) {
        self.trigger_log_unavailable = v;
    }

    /// Take get entries context.
    pub fn take_get_entries_context(&mut self) -> Option<GetEntriesContext> {
        self.get_entries_context.take()
    }

    /// makes a snapshot which can be retrieved with `snapshot()` and
    /// can be used to reconstruct the state at that point.
    /// If any configuration changes have been made since the last compaction,
    /// the result of the last apply_conf_change must be passed in.
    pub fn create_snapshot(
        &mut self,
        i: u64,
        cs: Option<ConfState>,
        data: &[u8],
    ) -> Result<Snapshot> {
        if i <= self.snapshot_metadata.index() {
            return Err(Error::Store(StorageError::SnapshotOutOfDate));
        }

        let offset = self.first_index();
        if i > self.last_index() {
            return Err(Error::Store(StorageError::SnapshotTemporarilyUnavailable));
        }

        self.snapshot_metadata.index = Some(i);
        self.snapshot_metadata.term = self.ents[(i - offset) as usize].term;
        if let Some(conf_state) = cs {
            self.raft_state.conf_state = conf_state;
        }

        let snapshot = Snapshot {
            metadata: Some(self.snapshot_metadata.clone()),
            data: Some(data.to_vec()),
        };
        Ok(snapshot)
    }
}

/// `MemStorage` is a thread-safe but incomplete implementation of `Storage`.
#[derive(Debug, Clone, Default)]
pub struct MemStorage {
    core: Arc<RwLock<MemStorageCore>>,
}

impl MemStorage {
    /// Returns a new memory storage value.
    pub fn new() -> Self {
        MemStorage {
            ..Default::default()
        }
    }

    /// Opens up a read lock on the storage and returns a gurad handle. Use this
    /// with functions that don't required mutation.
    pub async fn rl(&self) -> RwLockReadGuard<'_, MemStorageCore> {
        self.core.read().await
    }

    /// Opens up a write lock on the storage and returns gurad handle. Use this
    /// with functions that take a mutable reference to self.
    pub async fn wl(&self) -> RwLockWriteGuard<'_, MemStorageCore> {
        self.core.write().await
    }

    pub async fn apply_snapshot(&mut self, snap: Snapshot) -> Result<()> {
        let mut core = self.wl().await;
        core.apply_snapshot(snap)?;
        Ok(())
    }

    pub async fn compact(&mut self, compact_index: u64) -> Result<()> {
        let mut core = self.wl().await;
        core.compact(compact_index)?;
        Ok(())
    }

    pub async fn append(&mut self, ents: &[Entry]) -> Result<()> {
        let mut core = self.wl().await;
        core.append(ents)?;
        Ok(())
    }
}

#[async_trait]
impl Storage for MemStorage {
    async fn inital_state(&self) -> Result<RaftState> {
        Ok(self.rl().await.clone().raft_state)
    }

    async fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: Option<u64>,
        context: GetEntriesContext,
    ) -> Result<Vec<Entry>> {
        self.rl()
            .await
            .clone()
            .entries(low, high, max_size, context)
    }

    async fn term(&self, i: u64) -> Result<u64> {
        self.rl().await.clone().term(i)
    }

    async fn last_index(&self) -> Result<u64> {
        Ok(self.rl().await.clone().last_index())
    }

    async fn first_index(&self) -> Result<u64> {
        Ok(self.rl().await.clone().first_index())
    }

    async fn snapshot(&self, request_index: u64, _to: u64) -> Result<Snapshot> {
        let mut core = self.wl().await;
        if core.trigger_snap_unavailable {
            core.trigger_snap_unavailable = false;
            Err(Error::Store(StorageError::SnapshotTemporarilyUnavailable))
        } else {
            let mut snap = core.snapshot();
            let mut meta = snap.metadata.unwrap();
            if meta.index() < request_index {
                meta.index = Some(request_index);
            }
            snap.metadata = Some(meta);
            Ok(snap)
        }
    }
}

#[cfg(test)]
mod test {
    use std::panic::{self, AssertUnwindSafe};

    use prost::Message as PbMessage;

    use crate::errors::{Error as RaftError, StorageError};
    use raftpb::prelude::{ConfState, Entry, Snapshot, SnapshotMetadata};

    use super::{GetEntriesContext, MemStorage, MemStorageCore};

    fn new_entry(index: u64, term: u64) -> Entry {
        let mut e = Entry::default();
        e.term = Some(term);
        e.index = Some(index);
        e
    }

    fn size_of<T: PbMessage>(m: &T) -> u32 {
        m.encoded_len() as u32
    }

    fn new_snapshot(index: u64, term: u64, voters: Vec<u64>) -> Snapshot {
        let mut s = Snapshot::default();
        let mut meta = SnapshotMetadata::default();
        meta.index = Some(index);
        meta.term = Some(term);
        if let Some(ref mut conf_state) = meta.conf_state {
            conf_state.voters = voters;
        };
        s.metadata = Some(meta);
        s.data = Some((&[]).to_vec());
        s
    }

    #[test]
    fn test_storage_term() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let mut tests = vec![
            (2, Err(RaftError::Store(StorageError::Compacted))),
            (3, Ok(3)),
            (4, Ok(4)),
            (5, Ok(5)),
            (6, Err(RaftError::Store(StorageError::Unavailable))),
        ];

        for (i, (idx, wterm)) in tests.drain(..).enumerate() {
            let mut storage = MemStorageCore::new();
            storage.ents = ents.clone();

            let t = storage.term(idx);
            if t != wterm {
                panic!("#{}: expect res {:?}, got {:?}", i, wterm, t);
            }
        }
    }

    #[test]
    fn test_storage_entries() {
        let ents = vec![
            new_entry(3, 3),
            new_entry(4, 4),
            new_entry(5, 5),
            new_entry(6, 6),
        ];
        let max_u64 = u64::max_value();
        let mut tests = vec![
            (
                2,
                6,
                max_u64,
                Err(RaftError::Store(StorageError::Compacted)),
            ),
            (3, 4, max_u64, Ok(vec![new_entry(3, 3)])),
            (4, 5, max_u64, Ok(vec![new_entry(4, 4)])),
            (4, 6, max_u64, Ok(vec![new_entry(4, 4), new_entry(5, 5)])),
            (
                4,
                7,
                max_u64,
                Ok(vec![new_entry(4, 4), new_entry(5, 5), new_entry(6, 6)]),
            ),
            // even if maxsize is zero, the first entry should be returned
            (4, 7, 0, Ok(vec![new_entry(4, 4)])),
            // limit to 2
            (
                4,
                7,
                u64::from(size_of(&ents[1]) + size_of(&ents[2])),
                Ok(vec![new_entry(4, 4), new_entry(5, 5)]),
            ),
            (
                4,
                7,
                u64::from(size_of(&ents[1]) + size_of(&ents[2]) + size_of(&ents[3]) / 2),
                Ok(vec![new_entry(4, 4), new_entry(5, 5)]),
            ),
            (
                4,
                7,
                u64::from(size_of(&ents[1]) + size_of(&ents[2]) + size_of(&ents[3]) - 1),
                Ok(vec![new_entry(4, 4), new_entry(5, 5)]),
            ),
            // all
            (
                4,
                7,
                u64::from(size_of(&ents[1]) + size_of(&ents[2]) + size_of(&ents[3])),
                Ok(vec![new_entry(4, 4), new_entry(5, 5), new_entry(6, 6)]),
            ),
        ];
        for (i, (lo, hi, maxsize, wentries)) in tests.drain(..).enumerate() {
            let mut storage = MemStorageCore::new();
            storage.ents = ents.clone();
            let e = storage.entries(lo, hi, Some(maxsize), GetEntriesContext::empty(false));
            if e != wentries {
                panic!("#{}: expect entries {:?}, got {:?}", i, wentries, e);
            }
        }
    }

    #[test]
    fn test_storage_last_index() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let mut storage = MemStorageCore::new();
        storage.ents = ents;

        let wresult = 5;
        let result = storage.last_index();
        if result != wresult {
            panic!("want {:?}, got {:?}", wresult, result);
        }

        storage.append(&[new_entry(6, 5)]).unwrap();
        let wresult = 6;
        let result = storage.last_index();
        if result != wresult {
            panic!("want {:?}, got {:?}", wresult, result);
        }
    }

    #[test]
    fn test_storage_first_index() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let mut storage = MemStorageCore::new();
        storage.ents = ents;

        assert_eq!(storage.first_index(), 3);
        storage.compact(4).unwrap();
        assert_eq!(storage.first_index(), 4);
    }

    #[test]
    fn test_storage_compact() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let mut tests = vec![(2, 3, 3, 3), (3, 3, 3, 3), (4, 4, 4, 2), (5, 5, 5, 1)];
        for (i, (idx, windex, wterm, wlen)) in tests.drain(..).enumerate() {
            let mut storage = MemStorageCore::new();
            storage.ents = ents.clone();

            storage.compact(idx).unwrap();
            let index = storage.first_index();
            if index != windex {
                panic!("#{}: want {}, index {}", i, windex, index);
            }
            let term = if let Ok(v) =
                storage.entries(index, index + 1, Some(1), GetEntriesContext::empty(false))
            {
                v.first().map_or(0, |e| e.term())
            } else {
                0
            };
            if term != wterm {
                panic!("#{}: want {}, term {}", i, wterm, term);
            }
            let last = storage.last_index();
            let len = storage
                .entries(index, last + 1, Some(100), GetEntriesContext::empty(false))
                .unwrap()
                .len();
            if len != wlen {
                panic!("#{}: want {}, term {}", i, wlen, len);
            }
        }
    }

    #[test]
    fn test_storage_create_snapshot() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let nodes = vec![1, 2, 3];
        let mut conf_state = ConfState::default();
        conf_state.voters = nodes.clone();

        let unavailable = Err(RaftError::Store(
            StorageError::SnapshotTemporarilyUnavailable,
        ));
        let mut tests = vec![
            (4, Ok(new_snapshot(4, 4, nodes.clone()))),
            (5, Ok(new_snapshot(5, 5, nodes.clone()))),
            (6, unavailable),
        ];
        for (i, (idx, wresult)) in tests.drain(..).enumerate() {
            let mut storage = MemStorageCore::new();
            storage.ents = ents.clone();
            storage.raft_state.hard_state.commit = Some(idx);
            storage.raft_state.hard_state.term = Some(idx);
            storage.raft_state.conf_state = conf_state.clone();

            let result = storage.create_snapshot(idx, None, &[]);
            if result != wresult {
                panic!("#{}: want {:?}, got {:?}", i, wresult, result);
            }
        }
    }

    #[test]
    fn test_storage_append() {
        let ents = vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)];
        let mut tests = vec![
            (
                vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)],
                Some(vec![new_entry(3, 3), new_entry(4, 4), new_entry(5, 5)]),
            ),
            (
                vec![new_entry(3, 3), new_entry(4, 6), new_entry(5, 6)],
                Some(vec![new_entry(3, 3), new_entry(4, 6), new_entry(5, 6)]),
            ),
            (
                vec![
                    new_entry(3, 3),
                    new_entry(4, 4),
                    new_entry(5, 5),
                    new_entry(6, 5),
                ],
                Some(vec![
                    new_entry(3, 3),
                    new_entry(4, 4),
                    new_entry(5, 5),
                    new_entry(6, 5),
                ]),
            ),
            // overwrite compacted raft logs is not allowed
            (
                vec![new_entry(2, 3), new_entry(3, 3), new_entry(4, 5)],
                None,
            ),
            // truncate the existing entries and append
            (
                vec![new_entry(4, 5)],
                Some(vec![new_entry(3, 3), new_entry(4, 5)]),
            ),
            // direct append
            (
                vec![new_entry(6, 6)],
                Some(vec![
                    new_entry(3, 3),
                    new_entry(4, 4),
                    new_entry(5, 5),
                    new_entry(6, 6),
                ]),
            ),
        ];
        for (i, (entries, wentries)) in tests.drain(..).enumerate() {
            let mut storage = MemStorageCore::new();
            storage.ents = ents.clone();
            let res = panic::catch_unwind(AssertUnwindSafe(|| storage.append(&entries)));
            if let Some(wentries) = wentries {
                let _ = res.unwrap();
                let e = &storage.ents;
                if *e != wentries {
                    panic!("#{}: want {:?}, entries {:?}", i, wentries, e);
                }
            } else {
                res.unwrap_err();
            }
        }
    }

    #[test]
    fn test_storage_apply_snapshot() {
        let nodes = vec![1, 2, 3];
        let mut storage = MemStorageCore::new();

        // Apply snapshot successfully
        let snap = new_snapshot(4, 4, nodes.clone());
        storage.apply_snapshot(snap).unwrap();

        // Apply snapshot fails due to StorageError::SnapshotOutOfDate
        let snap = new_snapshot(3, 3, nodes);
        let e = storage.apply_snapshot(snap);
        println!("{:?}", e);
    }

    #[tokio::test]
    async fn test_async_storage_apply_snapshot() {
        let nodes = vec![1, 2, 3];
        let mut storage = MemStorage::new();

        // Apply snapshot successfully
        let snap = new_snapshot(4, 4, nodes.clone());
        storage.apply_snapshot(snap).await.unwrap();

        // Apply snapshot fails due to StorageError::SnapshotOutOfDate
        let snap = new_snapshot(3, 3, nodes);
        storage.apply_snapshot(snap).await.unwrap_err();
    }
}
