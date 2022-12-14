use crate::prelude::Entry;
use crate::HashSet;

use prost::Message as PbMessage;
use raftpb::raftpb::Message;
use slog::{b, record_static, OwnedKVList, Record, KV};

use std::fmt;
use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

/// A number to represent that there is no limit.
pub const NO_LIMIT: u64 = u64::MAX;

/// Truncates the list of entries down to a specific byte-length of
/// all entries together.
///
/// # Examples
///
/// ```
/// use raft::{util::limit_size, prelude::*};
///
/// let template = {
///     let mut entry = Entry::default();
///     entry.data = "*".repeat(100).into_bytes().into();
///     entry
/// };
///
/// // Make a bunch of entries that are ~100 bytes long
/// let mut entries = vec![
///     template.clone(),
///     template.clone(),
///     template.clone(),
///     template.clone(),
///     template.clone(),
/// ];
///
/// assert_eq!(entries.len(), 5);
/// limit_size(&mut entries, Some(220));
/// assert_eq!(entries.len(), 2);
///
/// // `entries` will always have at least 1 Message
/// limit_size(&mut entries, Some(0));
/// assert_eq!(entries.len(), 1);
/// ```
pub fn limit_size<T: PbMessage + Clone>(entries: &mut Vec<T>, max: Option<u64>) {
    if entries.len() <= 1 {
        return;
    }
    let max = match max {
        None | Some(NO_LIMIT) => return,
        Some(max) => max,
    };

    let mut size = 0;
    let limit = entries
        .iter()
        .take_while(|&e| {
            if size == 0 {
                size += e.encoded_len() as u64;
                return true;
            }
            size += e.encoded_len() as u64;
            size <= max
        })
        .count();

    entries.truncate(limit);
}

/// Check whether the entry is continuous to the message.
/// i.e msg's next entry index should be equal to the index of the first entry in `ents`
pub fn is_continuous_ents(msg: &Message, ents: &[Entry]) -> bool {
    if !msg.entries.is_empty() && !ents.is_empty() {
        let expected_next_idx = msg.entries.last().unwrap().index + 1;
        return expected_next_idx == ents.first().unwrap().index;
    }
    true
}

struct FormatKeyValueList {
    pub buffer: String,
}

impl slog::Serializer for FormatKeyValueList {
    fn emit_arguments(&mut self, key: slog::Key, val: &fmt::Arguments) -> slog::Result {
        if !self.buffer.is_empty() {
            write!(&mut self.buffer, ", {}: {}", key, val).unwrap();
        } else {
            write!(&mut self.buffer, "{}: {}", key, val).unwrap();
        }
        Ok(())
    }
}

pub(crate) fn format_kv_list(kv_list: &OwnedKVList) -> String {
    let mut formatter = FormatKeyValueList {
        buffer: "".to_owned(),
    };
    let record = record_static!(slog::Level::Trace, "");
    kv_list
        .serialize(
            &Record::new(&record, &format_args!(""), b!()),
            &mut formatter,
        )
        .unwrap();
    formatter.buffer
}

/// Get the majority number of given nodes count.
#[inline]
pub fn majority(total: usize) -> usize {
    (total / 2) + 1
}

pub(super) struct SwitchChannel<T> {
    ch: Option<async_channel::Receiver<T>>,
}

impl<T> SwitchChannel<T> {
    pub(super) fn new() -> SwitchChannel<T> {
        SwitchChannel { ch: None }
    }

    pub(super) fn set_ready(&mut self, ch: Option<async_channel::Receiver<T>>) {
        self.ch = ch
    }

    pub(super) fn has_ready(&self) -> bool {
        self.ch.is_some()
    }

    pub(super) fn recv(&self) -> SwitchChannelRecv<'_, T> {
        SwitchChannelRecv { p: self }
    }
}

pub(super) struct SwitchChannelRecv<'a, T> {
    p: &'a SwitchChannel<T>,
}

impl<T> Future for SwitchChannelRecv<'_, T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        if self.p.ch.is_none() {
            return Poll::Pending;
        }
        if let Some(ch) = self.p.ch.as_ref() {
            if let Poll::Ready(result) = Pin::new(&mut ch.recv()).poll(cx) {
                if let Ok(val) = result {
                    return Poll::Ready(val);
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// A convenient struct that handles queries to both HashSet.
pub struct Union<'a> {
    first: &'a HashSet<u64>,
    second: &'a HashSet<u64>,
}

impl<'a> Union<'a> {
    /// Creates a union.
    pub fn new(first: &'a HashSet<u64>, second: &'a HashSet<u64>) -> Union<'a> {
        Union { first, second }
    }

    /// Checks if id shows up in either HashSet.
    #[inline]
    pub fn contains(&self, id: u64) -> bool {
        self.first.contains(&id) || self.second.contains(&id)
    }

    /// Returns an iterator iterates the distinct values in two sets.
    pub fn iter(&self) -> impl Iterator<Item = u64> + '_ {
        self.first.union(self.second).cloned()
    }

    /// Checks if union is empty.
    pub fn is_empty(&self) -> bool {
        self.first.is_empty() && self.second.is_empty()
    }

    /// Gets the count of the union.
    ///
    /// The time complexity is O(n).
    pub fn len(&self) -> usize {
        // Usually, second is empty.
        self.first.len() + self.second.len() - self.second.intersection(self.first).count()
    }
}

/// Get the approximate size of entry
#[inline]
pub fn entry_approximate_size(e: &Entry) -> usize {
    //  message Entry {
    //      EntryType entry_type = 1;
    //      uint64 term = 2;
    //      uint64 index = 3;
    //      bytes data = 4;
    //      bytes context = 6;
    // }
    // Each field has tag(1 byte) if it's not default value.
    // Tips: x bytes can represent a value up to 1 << x*7 - 1,
    // So 1 byte => 127, 2 bytes => 16383, 3 bytes => 2097151.
    // If entry_type is normal(default), in general, the size should
    // be tag(4) + term(1) + index(2) + data(2) + context(1) = 10.
    // If entry_type is conf change, in general, the size should be
    // tag(5) + entry_type(1) + term(1) + index(2) + data(1) + context(1) = 11.
    // We choose 12 in case of large index or large data for normal entry.
    e.data.len() + e.context.len() + 12
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Runtime;

    fn new_runtime() -> Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn test_switch_channel() {
        new_runtime().block_on(async move {
            println!("{:?}", 0);
        });
    }
}
