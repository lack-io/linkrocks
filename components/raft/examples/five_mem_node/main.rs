// Copyright 2019 TiKV Project Authors. Licensed under Apache-2.0.

// We use `default` method a lot to be support prost and rust-protobuf at the
// same time. And reassignment can be optimized by compiler.
#![allow(clippy::field_reassign_with_default)]

use raft::config::Config;
use raft::prelude::{ConfChange, ConfChangeType, Entry, EntryType, Message, MessageType, Snapshot};
use raft::raft::StateType;
use raft::raw_node::RawNode;
use slog::{Drain, Logger};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, Mutex};
use tokio::time::Instant;

use prost::Message as PbMessage;
use raft::storage::MemStorage;
use regex::Regex;

use slog::{error, info, o};

#[tokio::main]
async fn main() {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain)
        .chan_size(4096)
        .overflow_strategy(slog_async::OverflowStrategy::Block)
        .build()
        .filter_level(slog::Level::Info)
        .fuse();
    let logger = slog::Logger::root(drain, o!());

    const NUM_NODES: u32 = 5;
    // Create 5 mailboxes to send/receive messages. Every node holds a `Receiver` to receive
    // messages from others, and uses the respective `Sender` to send messages to others.
    let (mut tx_vec, mut rx_vec) = (Vec::new(), Vec::new());
    for _ in 0..NUM_NODES {
        let (tx, rx) = broadcast::channel(10);
        tx_vec.push(tx);
        rx_vec.push(rx);
    }

    let (tx_stop, rx_stop) = broadcast::channel(10);
    let rx_stop = Arc::new(Mutex::new(rx_stop));

    // A global pending proposals queue. New proposals will be pushed back into the queue, and
    // after it's committed by the raft cluster, it will be poped from the queue.
    let proposals = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));

    // let mut handles = Vec::new();
    for (i, rx) in rx_vec.into_iter().enumerate() {
        // A map[peer_id -> sender]. In the example we create 5 nodes, with ids in [1, 5].
        let mailboxes = (1..6u64).zip(tx_vec.iter().cloned()).collect();
        let proposals = Arc::clone(&proposals);
        let rx_stop = Arc::clone(&rx_stop);

        // Clone the stop receiver
        // let rx_stop_clone = rx_stop;
        let logger = logger.clone();
        // Here we spawn the node on a new thread and keep a handle so we can join on them later.
        let handle = tokio::spawn(Box::pin(async move {
            to_do(i as u64, rx, mailboxes, &logger.clone(), proposals, rx_stop).await;
        }));
        // handles.push(handle);
    }

    // Propose some conf changes so that followers can be initialized.
    add_all_followers(proposals.as_ref()).await;

    // Put 100 key-value pairs.
    info!(
        logger,
        "We get a 5 nodes Raft cluster now, now propose 100 proposals"
    );

    for i in 0..100u16 {
        let (proposal, mut rx) = Proposal::normal(i, "hello, world".to_owned());
        proposals.lock().await.push_back(proposal);
        // After we got a response from `rx`, we can assume the put succeeded and following
        // `get` operations can find the key-value pair.
        rx.recv().await.unwrap();
    }

    info!(logger, "Propose 100 proposals success!");

    // Send terminate signals
    for _ in 0..NUM_NODES {
        let _ = tx_stop.send(Signal::Terminate);
    }

    // Wait for the thread to finish
    // for th in handles {
        // let _ = th.await;
    // }
}

async fn to_do(
    id: u64,
    rx: Receiver<Message>,
    mailboxes: HashMap<u64, Sender<Message>>,
    logger: &Logger,
    proposals: Arc<Mutex<VecDeque<Proposal>>>,
    rx_stop: Arc<Mutex<Receiver<Signal>>>,
) {
    tokio::time::sleep(Duration::from_secs(5)).await;
    // A map[peer_id -> sender]. In the example we create 5 nodes, with ids in [1, 5].
    let mut node = match id {
        // Peer 1 is the leader.
        0 => Node::create_raft_leader(1, rx, mailboxes, &logger).await,
        // Other peers are followers.
        _ => Node::create_raft_follower(rx, mailboxes).await,
    };
    // Tick the raft node per 100ms. So use an `Instant` to trace it.
    let mut t = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(10)).await;
        tokio::select! {
            val = node.my_mailbox.recv() => {
                match val {
                    Ok(msg) => node.step(msg, &logger).await,
                    Err(broadcast::error::RecvError::Closed) => {
                        return;
                    },
                    _ => {},
                }
            },
            _ = async {} => {}
        }

        if node.raft_group.is_none() {
            continue;
        }
        let raft_group = node.raft_group.as_mut().unwrap();

        if t.elapsed() >= Duration::from_millis(100) {
            // Tick the raft.
            raft_group.tick().await;
            t = Instant::now();
        }

        // Let the leader pick pending proposals from the global queue.
        if raft_group.raft.state == StateType::Leader {
            // Handle new proposals.
            let mut proposals = proposals.lock().await;
            for p in proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                propose(raft_group, p).await;
            }
        }

        // Handle readies from the raft.
        on_ready(
            raft_group,
            &mut node.kv_pairs,
            &node.mailboxes,
            &proposals,
            &logger,
        )
        .await;

        // Check control signals from
        let rx_stop = Arc::clone(&rx_stop);
        if check_signals(rx_stop).await {
            break;
        };
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Terminate,
}

async fn check_signals(receiver: Arc<Mutex<broadcast::Receiver<Signal>>>) -> bool {
    match receiver.lock().await.recv().await {
        Ok(Signal::Terminate) => true,
        // Err(TryRecvError::Empty) => false,
        // Err(TryRecvError::Disconnected) => true,
        Err(_) => todo!(),
    }
}

struct Node {
    // None if the raft is not initialized.
    raft_group: Option<RawNode<MemStorage>>,
    my_mailbox: Receiver<Message>,
    mailboxes: HashMap<u64, Sender<Message>>,
    // Key-value pairs after applied. `MemStorage` only contains raft logs,
    // so we need an additional storage engine.
    kv_pairs: HashMap<u16, String>,
}

impl Node {
    // Create a raft leader only with itself in its configuration.
    async fn create_raft_leader(
        id: u64,
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
        logger: &slog::Logger,
    ) -> Self {
        let mut cfg = example_config();
        cfg.id = id;
        let logger = logger.new(o!("tag" => format!("peer_{}", id)));
        let mut s = Snapshot::default();
        // Because we don't use the same configuration to initialize every node, so we use
        // a non-zero index to force new followers catch up logs by snapshot first, which will
        // bring all nodes to the same initial state.
        s.mut_metadata().index = 1;
        s.mut_metadata().term = 1;
        s.mut_metadata().mut_conf_state().voters = vec![1];
        let storage = MemStorage::new();
        storage.wl().await.apply_snapshot(s).unwrap();
        let raft_group = Some(RawNode::new(&cfg, storage, &logger).await.unwrap());
        Node {
            raft_group,
            my_mailbox,
            mailboxes,
            kv_pairs: Default::default(),
        }
    }

    // Create a raft follower.
    async fn create_raft_follower(
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
    ) -> Self {
        Node {
            raft_group: None,
            my_mailbox,
            mailboxes,
            kv_pairs: Default::default(),
        }
    }

    // Initialize raft for followers.
    async fn initialize_raft_from_message(&mut self, msg: &Message, logger: &slog::Logger) {
        if !is_initial_msg(msg) {
            return;
        }
        let mut cfg = example_config();
        cfg.id = msg.to;
        let logger = logger.new(o!("tag" => format!("peer_{}", msg.to)));
        let storage = MemStorage::new();
        self.raft_group = Some(RawNode::new(&cfg, storage, &logger).await.unwrap());
    }

    // Step a raft message, initialize the raft if need.
    async fn step(&mut self, msg: Message, logger: &slog::Logger) {
        if self.raft_group.is_none() {
            if is_initial_msg(&msg) {
                self.initialize_raft_from_message(&msg, logger).await;
            } else {
                return;
            }
        }
        let raft_group = self.raft_group.as_mut().unwrap();
        let _ = raft_group.step(msg);
    }
}

async fn on_ready(
    raft_group: &mut RawNode<MemStorage>,
    kv_pairs: &mut HashMap<u16, String>,
    mailboxes: &HashMap<u64, Sender<Message>>,
    proposals: &Mutex<VecDeque<Proposal>>,
    logger: &slog::Logger,
) {
    if !raft_group.has_ready().await {
        return;
    }
    let store = raft_group.raft.raft_log.store.clone();

    // Get the `Ready` with `RawNode::ready` interface.
    let mut ready = raft_group.ready().await;

    let handle_messages = |msgs: Vec<Message>| {
        for msg in msgs {
            let to = msg.to;
            if mailboxes[&to].send(msg).is_err() {
                error!(
                    logger,
                    "send raft message to {} fail, let Raft retry it", to
                );
            }
        }
    };

    if !ready.messages().is_empty() {
        // Send out the messages come from the node.
        handle_messages(ready.take_messages());
    }

    // Apply the snapshot. It's necessary because in `RawNode::advance` we stabilize the snapshot.
    if *ready.snapshot() != Snapshot::default() {
        let s = ready.snapshot().clone();
        if let Err(e) = store.wl().await.apply_snapshot(s) {
            error!(
                logger,
                "apply snapshot fail: {:?}, need to retry or panic", e
            );
            return;
        }
    }

    // Apply all committed entries.
    handle_committed_entries(
        &store,
        raft_group,
        kv_pairs,
        proposals,
        ready.take_committed_entries(),
    )
    .await;

    // Persistent raft logs. It's necessary because in `RawNode::advance` we stabilize
    // raft logs to the latest position.
    if let Err(e) = store.wl().await.append(ready.entries()) {
        error!(
            logger,
            "persist raft log fail: {:?}, need to retry or panic", e
        );
        return;
    }

    if let Some(hs) = ready.hs() {
        // Raft HardState changed, and we need to persist it.
        store.wl().await.set_hardstate(hs.clone());
    }

    if !ready.persisted_messages().is_empty() {
        // Send out the persisted messages come from the node.
        handle_messages(ready.take_persisted_messages());
    }

    // Call `RawNode::advance` interface to update position flags in the raft.
    let mut light_rd = raft_group.advance(ready).await;
    // Update commit index.
    if let Some(commit) = light_rd.commit_index() {
        store.wl().await.mut_hard_state().commit = commit;
    }
    // Send out the messages.
    handle_messages(light_rd.take_messages());
    // Apply all committed entries.
    handle_committed_entries(
        &store,
        raft_group,
        kv_pairs,
        proposals,
        light_rd.take_committed_entries(),
    )
    .await;
    // Advance the apply index.
    raft_group.advance_apply().await;
}

async fn handle_committed_entries(
    store: &MemStorage,
    rn: &mut RawNode<MemStorage>,
    kv_pairs: &mut HashMap<u16, String>,
    proposals: &Mutex<VecDeque<Proposal>>,
    committed_entries: Vec<Entry>,
) {
    for entry in committed_entries {
        if entry.data.is_empty() {
            // From new elected leaders.
            continue;
        }
        if let EntryType::EntryConfChange = entry.entry_type() {
            // For conf change messages, make them effective.
            let mut cc = ConfChange::default();
            cc.merge_length_delimited(&*entry.data).unwrap();
            let cs = rn.apply_conf_change(&cc).await.unwrap();
            store.wl().await.set_conf_state(cs);
        } else {
            // For normal proposals, extract the key-value pair and then
            // insert them into the kv engine.
            let data = std::str::from_utf8(entry.data.as_slice()).unwrap();
            let reg = Regex::new("put ([0-9]+) (.+)").unwrap();
            if let Some(caps) = reg.captures(data) {
                kv_pairs.insert(caps[1].parse().unwrap(), caps[2].to_string());
            }
        }
        if rn.raft.state == StateType::Leader {
            // The leader should response to the clients, tell them if their proposals
            // succeeded or not.
            let proposal = proposals.lock().await.pop_front().unwrap();
            proposal.propose_success.send(true).unwrap();
        }
    }
}

fn example_config() -> Config {
    Config {
        election_tick: 10,
        heartbeat_tick: 3,
        ..Default::default()
    }
}

// The message can be used to initialize a raft node or not.
fn is_initial_msg(msg: &Message) -> bool {
    let msg_type = msg.get_msg_type();
    msg_type == MessageType::MsgVote
        || msg_type == MessageType::MsgPreVote
        || (msg_type == MessageType::MsgHeartbeat && msg.commit == 0)
}

struct Proposal {
    normal: Option<(u16, String)>, // key is an u16 integer, and value is a string.
    conf_change: Option<ConfChange>, // conf change.
    transfer_leader: Option<u64>,
    // If it's proposed, it will be set to the index of the entry.
    proposed: u64,
    propose_success: Sender<bool>,
}

impl Proposal {
    fn conf_change(cc: &ConfChange) -> (Self, Receiver<bool>) {
        let (tx, rx) = broadcast::channel(1);
        let proposal = Proposal {
            normal: None,
            conf_change: Some(cc.clone()),
            transfer_leader: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }

    fn normal(key: u16, value: String) -> (Self, Receiver<bool>) {
        let (tx, rx) = broadcast::channel(1);
        let proposal = Proposal {
            normal: Some((key, value)),
            conf_change: None,
            transfer_leader: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }
}

async fn propose(raft_group: &mut RawNode<MemStorage>, proposal: &mut Proposal) {
    let last_index1 = raft_group.raft.raft_log.last_index().await + 1;
    if let Some((ref key, ref value)) = proposal.normal {
        let data = format!("put {} {}", key, value).into_bytes();
        let _ = raft_group.propose(vec![], data);
    } else if let Some(ref cc) = proposal.conf_change {
        let _ = raft_group.propose_conf_change(vec![], Box::new(cc.clone()));
    } else if let Some(_transferee) = proposal.transfer_leader {
        // TODO: implement transfer leader.
        unimplemented!();
    }

    let last_index2 = raft_group.raft.raft_log.last_index().await + 1;
    if last_index2 == last_index1 {
        // Propose failed, don't forget to respond to the client.
        proposal.propose_success.send(false).unwrap();
    } else {
        proposal.proposed = last_index1;
    }
}

// Proposes some conf change for peers [2, 5].
async fn add_all_followers(proposals: &Mutex<VecDeque<Proposal>>) {
    for i in 2..6u64 {
        let mut conf_change = ConfChange::default();
        conf_change.node_id = i;
        conf_change.set_change_type(ConfChangeType::ConfChangeAddNode);
        loop {
            let (proposal, mut rx) = Proposal::conf_change(&conf_change);
            proposals.lock().await.push_back(proposal);
            if rx.recv().await.unwrap() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
