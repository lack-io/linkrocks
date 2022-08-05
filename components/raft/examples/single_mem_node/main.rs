use std::{collections::HashMap, sync::Arc, time::Duration};

use raft::{
    config::Config,
    prelude::{ConfState, Entry, EntryType, Message},
    raw_node::RawNode,
    storage::MemStorage,
};
use slog::{error, info, o, Drain, Logger};
use tokio::{sync::broadcast, time::Instant};

type ProposeCallback = Arc<Box<dyn Fn()>>;

#[derive(Clone)]
enum Msg {
    Propose {
        id: u8,
        cb: ProposeCallback,
    },
    // Here we don't use Raft Message, so use dead_code to
    // avoid the compiler warning.
    #[allow(dead_code)]
    Raft(Message),
}

unsafe impl Send for Msg {}

// A simple example about how to use the Raft library in Rust.
#[tokio::main]
async fn main() {
    // Create a storage for Raft, and here we just use a simple memory storage.
    // You need to build your own persistent storage in your production.
    // Please check the Storage trait in src/storage.rs to see how to implement one.
    let storage = MemStorage::new_with_conf_state(ConfState::from((vec![1], vec![]))).await;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain)
        .chan_size(4096)
        .overflow_strategy(slog_async::OverflowStrategy::Block)
        .build()
        .fuse();
    let logger = slog::Logger::root(drain, o!("tag" => format!("[{}]", 1)));

    // Create the configuration for the Raft node.
    let cfg = Config {
        // The unique ID for the Raft node.
        id: 1,
        // Election tick is for how long the follower may campaign again after
        // it doesn't receive any message from the leader.
        election_tick: 10,
        // Heartbeat tick is for how long the leader needs to send
        // a heartbeat to keep alive.
        heartbeat_tick: 3,
        // The max size limits the max size of each appended message. Mostly, 1 MB is enough.
        max_size_per_msg: 1024 * 1024 * 1024,
        // Max inflight msgs that the leader sends messages to follower without
        // receiving ACKs.
        max_inflight_msgs: 256,
        // The Raft applied index.
        // You need to save your applied index when you apply the committed Raft logs.
        applied: 0,
        ..Default::default()
    };

    // Create the Raft node.
    let mut node = RawNode::new(&cfg, storage, &logger).await.unwrap();

    let (sender, mut receiver) = broadcast::channel(1);

    // Use another thread to propose a Raft request.
    // let sender_clone = sender.clone();
    let logger1 = logger.clone();
    let _ = tokio::spawn(async move {
        let logger_clone = logger1.clone();
        send_propose(logger_clone, &sender).await;
        let logger_clone = logger1.clone();
        send_propose(logger_clone, &sender).await;
        drop(sender);
    });

    // Loop forever to drive the Raft.
    let mut t = Instant::now();
    let mut timeout = Duration::from_millis(100);

    // Use a HashMap to hold the `propose` callbacks.
    let mut cbs = HashMap::new();

    loop {
        tokio::select! {
            val = receiver.recv() => {
                match val {
                    Ok(Msg::Propose { id, cb }) => {
                        cbs.insert(id, cb);
                        if let Err(e) = node.propose(vec![], vec![id]).await {
                            error!(logger, "propose {:?}", e);
                            return ;
                        };
                    }
                    Ok(Msg::Raft(m)) => node.step(m).await.unwrap(),
                    Err(broadcast::error::RecvError::Closed) => {
                        return;
                    },
                    _ => {},
                }
            },
            _ = async {} => {}
        }

        let d = t.elapsed();
        t = Instant::now();
        if d >= timeout {
            timeout = Duration::from_millis(100);
            // We drive Raft every 100ms.
            node.tick().await;
        } else {
            timeout -= d;
        }
        on_ready(&mut node, &mut cbs).await;
    }
}

async fn on_ready(raft_group: &mut RawNode<MemStorage>, cbs: &mut HashMap<u8, ProposeCallback>) {
    if !raft_group.has_ready().await {
        return;
    }
    let store = raft_group.raft.raft_log.store.clone();

    // Get the `Ready` with `RawNode::ready` interface.
    let mut ready = raft_group.ready().await;

    let handle_messages = |msgs: Vec<Message>| {
        for _msg in msgs {
            // Send messages to other peers.
        }
    };

    if !ready.messages().is_empty() {
        // Send out the messages come from the node.
        handle_messages(ready.take_messages());
    }

    if !ready.snapshot().is_empty() {
        // This is a snapshot, we need to apply the snapshot at first.
        store
            .wl()
            .await
            .apply_snapshot(ready.snapshot().clone())
            .unwrap();
    }

    let mut _last_apply_index = 0;
    let mut handle_committed_entries = |committed_entries: Vec<Entry>| {
        for entry in committed_entries {
            // Mostly, you need to save the last apply index to resume applying
            // after restart. Here we just ignore this because we use a Memory storage.
            _last_apply_index = entry.index;

            if entry.data.is_empty() {
                // Emtpy entry, when the peer becomes Leader it will send an empty entry.
                continue;
            }

            if entry.entry_type() == EntryType::EntryNormal {
                if let Some(cb) = cbs.remove(entry.data.get(0).unwrap()) {
                    cb();
                }
            }

            // TODO: handle EntryConfChange
        }
    };
    handle_committed_entries(ready.take_committed_entries());

    if !ready.entries().is_empty() {
        // Append entries to the Raft log.
        store.wl().await.append(ready.entries()).unwrap();
    }

    if let Some(hs) = ready.hs() {
        // Raft HardState changed, and we need to persist it.
        store.wl().await.set_hardstate(hs.clone());
    }

    if !ready.persisted_messages().is_empty() {
        // Send out the persisted messages come from the node.
        handle_messages(ready.take_persisted_messages());
    }

    // Advance the Raft.
    let mut light_rd = raft_group.advance(ready).await;
    // Update commit index.
    if let Some(commit) = light_rd.commit_index() {
        store.wl().await.mut_hard_state().set_commit(commit);
    }
    // Send out the messages.
    handle_messages(light_rd.take_messages());
    // Apply all committed entries.
    handle_committed_entries(light_rd.take_committed_entries());
    // Advance the apply index.
    raft_group.advance_apply().await;
}

async fn send_propose(logger: Logger, sender: &broadcast::Sender<Msg>) {
    // Wait some time and send the request to the Raft.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let (s1, mut r1) = broadcast::channel::<u8>(1);

    info!(logger, "propose a request");

    // Send a command to the Raft, wait for the Raft to apply it
    // and get the result.
    let _ = sender.send(Msg::Propose {
        id: 1,
        cb: Arc::new(Box::new(move || {
            s1.send(3).unwrap();
        })),
    });

    let n = r1.recv().await;
    // println!("{:?}", n)

    info!(logger, "receive the propose callback: {:?}", n);
}
