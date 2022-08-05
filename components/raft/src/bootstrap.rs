use std::sync::Arc;

use slog::Logger;
use tokio::sync::Mutex;

use crate::{
    config::Config,
    errors::{Error, Result},
    node::{AsyncNode, Node},
    raw_node::{Peer, RawNode},
    storage::Storage,
};

pub async fn start_node<'a, T>(
    c: &'a Config,
    store: T,
    peers: &'a [Peer],
    logger: &'a Logger,
) -> Result<Box<dyn Node>>
where
    T: Storage + Sync + Send + 'static,
{
    if peers.len() == 0 {
        return Err(Error::ConfigInvalid(
            "no peers given; use RestartNode instead".to_owned(),
        ));
    }

    let mut rn = RawNode::new(c, store, &logger).await?;
    rn.boot_strap(peers).await?;
    let node = AsyncNode::new(rn);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.spawn(async move { node.run().await });

    Ok(Box::new(node))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use raftpb::raftpb::ConfState;

    use crate::{config::Config, default_logger, raw_node::Peer, storage::MemStorage};

    use super::start_node;

    #[tokio::test]
    async fn test_start_node() {
        let config = Config {
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
        let peer = Peer {
            id: 1,
            context: Some(vec![]),
        };
        let logger = default_logger();
        let storage = MemStorage::new();
        let mut node = start_node(&config, storage, &[peer], &logger)
            .await
            .unwrap();

        node.clone().lock().await.stop().await;
    }

    async fn to_do_some_thing() {
        println!("todo!")
    }

    #[tokio::test]
    async fn async_task() {
        to_do_some_thing().await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
