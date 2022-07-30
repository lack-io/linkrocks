use std::cmp::Ordering;

/// A buffer of inflight messages.
#[derive(Debug, PartialEq, Clone)]
pub struct Inflights {
    // the starting index in the buffer
    start: usize,
    // number of inflights in the buffer
    count: usize,

    // ring buffer
    buffer: Vec<u64>,

    // capacity
    cap: usize,

    // To support dynamically change inflight size.
    incoming_cap: Option<usize>,
}

impl Inflights {
    /// Creates a new buffer fir inflight messages.
    pub fn new(cap: usize) -> Inflights {
        Inflights {
            start: 0,
            count: 0,
            buffer: Vec::with_capacity(cap),
            cap,
            incoming_cap: None,
        }
    }

    /// Adjust inflight buffer capacity. Set it to `0` will disable the progress.
    /// Calling it between `self.full()` and `self.add()` can cause a panic.
    pub fn set_cap(&mut self, incoming_cap: usize) {
        match self.cap.cmp(&incoming_cap) {
            Ordering::Equal => self.incoming_cap = None,
            Ordering::Less => {
                if self.start + self.count <= self.cap {
                    if self.buffer.capacity() > 0 {
                        self.buffer.reserve(incoming_cap - self.buffer.len())
                    }
                } else {
                    debug_assert_eq!(self.cap, self.buffer.len());
                    let mut buffer = Vec::with_capacity(incoming_cap);
                    buffer.extend_from_slice(&self.buffer[self.start..]);
                    buffer
                        .extend_from_slice(&&self.buffer[0..self.count - (self.cap - self.start)]);
                    self.buffer = buffer;
                    self.start = 0;
                }
                self.cap = incoming_cap;
                self.incoming_cap = None;
            }
            Ordering::Greater => {
                if self.count == 0 {
                    self.cap = incoming_cap;
                    self.incoming_cap = None;
                    self.start = 0;
                    if self.buffer.capacity() > 0 {
                        self.buffer = Vec::with_capacity(incoming_cap);
                    }
                } else {
                    self.incoming_cap = Some(incoming_cap);
                }
            }
        }
    }

    #[inline]
    pub fn full(&self) -> bool {
        self.count == self.cap || self.incoming_cap.map_or(false, |cap| self.count >= cap)
    }
}
