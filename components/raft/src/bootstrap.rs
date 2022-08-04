use raftpb::raftpb::ConfState;
use slog::o;

use crate::{
    config::Config,
    errors::{Error, Result},
    raw_node::{Peer, RawNode},
    storage::{MemStorage, Storage}, node::{Node, AsyncNode}, default_logger,
};

pub async fn start_node<'a, T: Storage>(
    c: &'a Config,
    peers: &'a [Peer],
) -> Result<Box<dyn Node + Send + Sync>> {
    if peers.len() == 0 {
        return Err(Error::ConfigInvalid(
            "no peers given; use RestartNode instead".to_owned(),
        ));
    }

    let storage = MemStorage::new_with_conf_state(ConfState::from((vec![1], vec![]))).await;

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain)
        .chan_size(4096)
        .overflow_strategy(slog_async::OverflowStrategy::Block)
        .build()
        .fuse();
    let logger = slog::Logger::root(drain, o!("tag" => format!("[{}]", 1)));
    let logger = default_logger();

    let rn = RawNode::new(c, storage, &logger).await?;

    let node = AsyncNode {
        propc: todo!(),
        recvc: todo!(),
        confc: todo!(),
        conf_statec: todo!(),
        readyc: todo!(),
        advancec: todo!(),
        tickc: todo!(),
        done: todo!(),
        stop: todo!(),
        status: todo!(),
        rn,
    };

    Ok(node)
}
