use std::collections::VecDeque;

use crate::prelude::Message;
use crate::{fatal, HashMap, HashSet};
use slog::Logger;

/// Determines the relative safety of and consistency of read only requests.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ReadOnlyOption {
    /// Safe guarantees the linearizability of the read only request by
    /// communicating with the quorum. It is the default and suggested option.
    Safe,
    /// LeaseBased ensures linearizability of the read only request by
    /// relying on the leader lease. It can be affected by clock drift.
    /// If the clock drift is unbounded, leader might keep the lease longer than it
    /// should (clock can move backward/pause without any bound). ReadIndex is not safe
    /// in that case.
    LeaseBased,
}

impl Default for ReadOnlyOption {
    fn default() -> ReadOnlyOption {
        ReadOnlyOption::Safe
    }
}

// ReadState provides state for read only query.
// It's caller's responsibility to call ReadIndex first before getting
// this state from ready, it's also caller's duty to differentiate if this
// state is what it requests through request_ctx, eg. given a unique id as
// request_ctx
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReadState {
    pub index: u64,
    pub request_ctx: Vec<u8>,
}

#[derive(Debug, Default, Clone)]
pub struct ReadIndexStatus {
    pub req: Message,
    pub index: u64,
    pub acks: HashSet<u64>,
}

#[derive(Debug, Default, Clone)]
pub struct ReadOnly {
    pub option: ReadOnlyOption,
    pub pending_read_index: HashMap<Vec<u8>, ReadIndexStatus>,
    pub read_index_queue: VecDeque<Vec<u8>>,
}

impl ReadOnly {
    pub fn new(option: ReadOnlyOption) -> ReadOnly {
        ReadOnly {
            option,
            pending_read_index: HashMap::default(),
            read_index_queue: VecDeque::new(),
        }
    }

    /// Add a read only request into readonly struct.
    ///
    /// `index` is the commit index of the raft state machine when it received
    /// the read only request.
    ///
    /// `m` is the original read only request message from the local or remote node.
    pub fn add_request(&mut self, index: u64, req: Message, self_id: u64) {
        let ctx = {
            let key = &req.entries[0].data;
            if self.pending_read_index.contains_key(key) {
                return;
            }
            key.to_vec()
        };
        let mut acks = HashSet::<u64>::default();
        acks.insert(self_id);
        let status = ReadIndexStatus { req, index, acks };
        self.pending_read_index.insert(ctx.clone(), status);
        self.read_index_queue.push_back(ctx);
    }

    /// Notifies the ReadOnly struct that the raft state machine received
    /// an acknownledgment of the heartbeat that attached with the read only request
    /// context.
    pub fn recv_ack(&mut self, id: u64, ctx: &[u8]) -> Option<&HashSet<u64>> {
        self.pending_read_index.get_mut(ctx).map(|rs| {
            rs.acks.insert(id);
            &rs.acks
        })
    }

    /// Advances the read only request queue kept by the ReadOnly struct.
    /// It dequeses the requests until it finds the read only request that has
    /// the same context as the given `ctx`.
    pub fn advance(&mut self, ctx: &[u8], logger: &Logger) -> Vec<ReadIndexStatus> {
        let mut rss = vec![];
        if let Some(i) = self.read_index_queue.iter().position(|x| {
            if !self.pending_read_index.contains_key(x) {
                fatal!(logger, "cannot find correspond read state from pending map");
            }
            *x == ctx
        }) {
            for _ in 0..=i {
                let rs = self.read_index_queue.pop_front().unwrap();
                let status = self.pending_read_index.remove(&rs).unwrap();
                rss.push(status);
            }
        }
        rss
    }

    /// Returns the context of the last pending read only request in ReadOnly struct.
    pub fn last_pending_request_ctx(&self) -> Option<Vec<u8>> {
        self.read_index_queue.back().cloned()
    }

    #[inline]
    pub fn pending_read_count(&self) -> usize {
        self.read_index_queue.len()
    }
}
