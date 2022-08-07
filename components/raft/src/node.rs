use std::{pin::Pin, time::Duration};

use crate::{
    errors::{Error, Result},
    raw_node::{conf_change_to_msg, is_local_msg, is_response_msg, RawNode, Ready, SnapshotStatus},
    status::Status,
    storage::Storage,
    util::SwitchChannel,
    INVALID_ID,
};

use raftpb::{
    raftpb::{ConfChangeV2, ConfState, Entry, Message, MessageType},
    ConfChangeI,
};
use slog::{info, warn};
use tokio::sync::watch;
use tokio_context::context::Context;

use async_trait::async_trait;

/// Node represents a node in raft cluster.
#[async_trait]
pub trait Node: {
    /// tick increments the internal logical clock for the Node by a single tick. Election
    /// timeouts and heartbeat timeouts are in units of ticks.
    async fn tick(&mut self);

    /// campaign causes the Node to transition to candidate state and start campaigning to become leader.
    async fn campaign(&self, ctx: &mut Context) -> Result<()>;

    /// proposes that data be appended to the log. Note that proposals can be lost without
    /// notice, therefore it is user's job to ensure proposal retries.
    async fn propose(&self, ctx: &mut Context, data: &[u8]) -> Result<()>;

    /// proposes a configuration change. Like any proposal, the
    /// configuration change may be dropped with or without an error being
    /// retruned. In particular, configuration changes are dropped unless the
    /// leader has certainty that there is no prior unapplied configuration
    /// change in its log.
    async fn proposal_conf_change(
        &self,
        ctx: &mut Context,
        context: Vec<u8>,
        cc: Box<dyn ConfChangeI + Send>,
    ) -> Result<()>;

    /// advances the state machine using the given message.
    async fn step(&self, ctx: &mut Context, m: Message) -> Result<()>;

    /// returns a channel that returns the current point-in-time state.
    /// Users of the Node must call advance after retrieving the state retruned by Ready.
    ///
    /// Note: No committe entries from the next Ready may be applied until all committed entries
    /// and snapshots from the previous one have finished.
    async fn ready(&mut self) -> async_channel::Receiver<Ready>;

    /// notifies the Node that the application has saved progress up to the last Ready.
    /// It prepares the node to return the next available Ready.
    ///
    /// The application should generally call advance after applies the entries in last Ready.
    ///
    /// However, as an optimization, the application may call `advance` while it is applying the
    /// commands. For example, when the last Ready contains a snapshot, the application might take
    /// a long time to apply the snapshot data. To continue receiving Ready without blocking raft
    /// progress, it can call `advance` before finishing applying the last ready.
    async fn advance(&mut self);

    /// applies a config change (previously passed to `propose_conf_change`) to the node. This muse be
    /// called whenever a config change is observed in `Ready.committed_entries`, expcept when the app
    /// decides to reject the configuration change (i.e. treats it as a noop instead), in which case it
    /// must bot be called.
    async fn apply_conf_change(&self, cc: Box<dyn ConfChangeI + Send>) -> ConfState;

    /// attempts to transfer leadership to be given transferee.
    async fn transfer_leadership(&self, ctx: &mut Context, lead: u64, transfer: u64);

    /// requests a read state. The read state will be set in the ready.
    /// Read state has a read index. Once the application advances further than the read
    /// index, any linerizable read requests issued before the read request can be
    /// processed safely. The read state will have the same rctx attached.
    /// Note that request can be lost without notice, therefore it is user's job
    /// to ensure read index retries.
    async fn read_index(&self, ctx: &mut Context, rctx: &[u8]) -> Result<()>;

    /// retruns the current status of the raft state machine
    async fn status(&self) -> Pin<Box<Status>>;

    /// reports the given node is not reachable for the last send.
    async fn report_unreachable(&self, id: u64);

    /// reports the status of the sent snapshot. The id is the raft ID of the follower
    /// who is meant to receive the snapshot, and the status is SnapshotFinish or SnapshotFailure.
    /// Calling ReportSnapshot with SnapshotFinish is a no-op. But, any failure in applying a
    /// snapshot (for e.g., while streaming it from leader to follower), should be reported to the
    /// leader with SnapshotFailure. When leader sends a snapshot to a follower, it pauses any raft
    /// log probes until the follower can apply the snapshot and advance its state. If the follower
    /// can't do that, for e.g., due to a crash, it could end up in a limbo, never getting any
    /// updates from the leader. Therefore, it is crucial that the application ensures that any
    /// failure in snapshot sending is caught and reported back to the leader; so it can resume raft
    /// log probing in the follower.
    async fn report_snapshot(&self, id: u64, status: SnapshotStatus);

    /// Stop performs any necessary termination of the Node.
    async fn stop(&mut self);
}

#[derive(Default)]
pub(super) struct MsgWithResult {
    pub(super) m: Message,
    pub(super) result: Option<async_channel::Sender<Result<()>>>,
}

pub(super) struct AsyncNode<'a, T: Storage> {
    // proprose channel
    propc: (
        async_channel::Sender<MsgWithResult>,
        async_channel::Receiver<MsgWithResult>,
    ),
    recvc: (
        async_channel::Sender<Message>,
        async_channel::Receiver<Message>,
    ),
    confc: (
        async_channel::Sender<ConfChangeV2>,
        async_channel::Receiver<ConfChangeV2>,
    ),
    conf_statec: (
        async_channel::Sender<ConfState>,
        async_channel::Receiver<ConfState>,
    ),
    readyc: (async_channel::Sender<Ready>, async_channel::Receiver<Ready>),
    advancec: (async_channel::Sender<()>, async_channel::Receiver<()>),
    tickc: (async_channel::Sender<()>, async_channel::Receiver<()>),
    done: watch::Receiver<()>,
    // stopc: watch::Sender<()>,
    stop: (async_channel::Sender<()>, async_channel::Receiver<()>),
    status: (
        async_channel::Sender<async_channel::Sender<Status<'a>>>,
        async_channel::Receiver<async_channel::Sender<Status<'a>>>,
    ),
    rn: RawNode<T>,
}

#[async_trait]
impl<T: Storage + Send + Sync> Node for AsyncNode<'_, T> {
    async fn tick(&mut self) {
        let mut donec = self.done.clone();
        let timer = tokio::time::sleep(Duration::from_millis(10));
        tokio::select! {
            _ = self.tickc.0.send(()) => {}
            _ = donec.changed() => {}
            _ = timer => {
                warn!(self.rn.raft.logger, "{id} A tick missed to fire. Node blocks too long!", id = self.rn.raft.id);
            }
        }
    }

    async fn campaign(&self, ctx: &mut Context) -> Result<()> {
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgHup);
        self.step_no_wait(ctx, msg).await
    }

    async fn propose(&self, ctx: &mut Context, data: &[u8]) -> Result<()> {
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgProp);
        let mut entry = Entry::default();
        entry.set_data(data.to_vec());
        msg.set_entries(vec![entry]);
        self.step_wait(ctx, msg).await
    }

    async fn proposal_conf_change(
        &self,
        ctx: &mut Context,
        context: Vec<u8>,
        cc: Box<dyn ConfChangeI + Send>,
    ) -> Result<()> {
        let msg = conf_change_to_msg(context, cc);
        self.step_no_wait(ctx, msg).await
    }

    async fn step(&self, ctx: &mut Context, m: Message) -> Result<()> {
        if is_local_msg(m.get_msg_type()) {
            return Ok(());
        }
        self.step_no_wait(ctx, m).await
    }

    async fn ready(&mut self) -> async_channel::Receiver<Ready> {
        self.readyc.1.clone()
    }

    async fn advance(&mut self) {
        let mut donec = self.done.clone();
        tokio::select! {
            _ = self.advancec.0.send(()) => {},
            _ = donec.changed() => {},
        }
    }

    async fn apply_conf_change(&self, cc: Box<dyn ConfChangeI + Send>) -> ConfState {
        let mut conf_state = ConfState::default();
        let mut donec = self.done.clone();
        tokio::select! {
            _ = self.confc.0.send(cc.as_v2().into_owned()) => {}
            _ = donec.changed() => {}
        }
        tokio::select! {
            cs = self.conf_statec.1.recv() => {
                conf_state = cs.unwrap();
            }
            _ = donec.changed() => {}
        }

        conf_state
    }

    /// attempts to transfer leadership to be given transferee.
    async fn transfer_leadership(&self, ctx: &mut Context, lead: u64, transfer: u64) {
        let mut donec = self.done.clone();
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgTransferLeader);
        msg.set_from(transfer);
        msg.set_to(lead);
        tokio::select! {
            _ = self.recvc.0.send(msg) => {},
            _ = donec.changed() => {},
            _ = ctx.done() => {}
        }
    }

    async fn read_index(&self, ctx: &mut Context, rctx: &[u8]) -> Result<()> {
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgReadIndex);
        let mut entry = Entry::default();
        entry.set_data(rctx.to_vec());
        msg.set_entries(vec![entry]);
        self.step(ctx, msg).await
    }

    /// retruns the current status of the raft state machine
    async fn status(&self) -> Pin<Box<Status<'_>>> {
        Box::pin(self.get_status().await)
    }

    /// reports the given node is not reachable for the last send.
    async fn report_unreachable(&self, id: u64) {
        let mut donec = self.done.clone();
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgUnreachable);
        msg.set_from(id);
        tokio::select! {
            _ = self.recvc.0.send(msg) => {}
            _ = donec.changed() => {},
        }
    }

    async fn report_snapshot(&self, id: u64, status: SnapshotStatus) {
        let rej = status == SnapshotStatus::Failure;
        let mut donec = self.done.clone();
        let mut msg = Message::default();
        msg.set_msg_type(MessageType::MsgSnapStatus);
        msg.set_from(id);
        msg.set_reject(rej);
        tokio::select! {
            _ = self.recvc.0.send(msg) => {},
            _ = donec.changed() => {}
        }
    }

    async fn stop(&mut self) {
        let mut donec = self.done.clone();
        tokio::select! {
            // Not already stopped, so tigger it
            _ = self.stop.0.send(()) => {}
            // Node has already been stopped - no need to do anything
            _ = donec.changed() => {
                return;
            }
        };
        // Block until the stop has been acknownledged by run()
        let _ = donec.changed().await;
    }
}

impl<'a, T: Storage> AsyncNode<'a, T> {
    pub(super) fn new(rn: RawNode<T>) -> AsyncNode<'a, T> {
        let (_, rx_done) = watch::channel(());
        let node = AsyncNode {
            propc: async_channel::bounded::<MsgWithResult>(1),
            recvc: async_channel::bounded::<Message>(1),
            confc: async_channel::bounded::<ConfChangeV2>(1),
            conf_statec: async_channel::bounded::<ConfState>(1),
            readyc: async_channel::bounded::<Ready>(1),
            advancec: async_channel::bounded::<()>(1),
            tickc: async_channel::bounded::<()>(128),
            done: rx_done,
            stop: async_channel::bounded::<()>(1),
            status: async_channel::bounded(1),
            rn,
        };

        node
    }

    pub(super) async fn run(&mut self) {
        let mut rd = Ready::default();
        let mut propc: SwitchChannel<MsgWithResult> = SwitchChannel::new();
        let mut readyc: SwitchChannel<Ready> = SwitchChannel::new();
        let mut advancec: SwitchChannel<()> = SwitchChannel::new();
        let mut donec = self.done.clone();

        let mut lead = INVALID_ID;

        loop {
            if advancec.has_ready() {
                readyc.set_ready(None);
            } else if self.rn.has_ready().await {
                rd = self.rn.ready().await;
                readyc.set_ready(Some(self.readyc.1.clone()));
            }

            if lead != self.rn.raft.leader_id {
                if self.rn.has_ready().await {
                    if lead == INVALID_ID {
                    } else {
                    }
                    propc.set_ready(Some(self.propc.1.clone()));
                } else {
                    propc.set_ready(None);
                }
                lead = self.rn.raft.leader_id;
            }

            tokio::select! {
                pm = propc.recv() => {
                    let mut m = pm.m;
                    m.set_from(self.rn.raft.id);
                    let r = self.rn.raft.step(m).await;
                    if let Some(result) = &pm.result {
                        let _ = result.send(r).await;
                        result.close();
                    }
                },
                Ok(m) = self.recvc.1.recv() => {
                    // filter out response message from unknown From.
                    if let Some(_) = self.rn.raft.prs().progress().get(&m.from) {
                        if is_response_msg(m.get_msg_type()) {
                            let _ = self.rn.raft.step(m).await;
                        }
                    }
                },
                Ok(cc) = self.confc.1.recv() => {
                    let ok_before = self.rn.raft.prs().progress().contains_key(&self.rn.raft.id);
                    let cs = self.rn.raft.apply_conf_change(&cc).await.unwrap();
                    let ok_after = self.rn.raft.prs().progress().contains_key(&self.rn.raft.id);
                    if ok_before && !ok_after {
                        let mut found = false;
                        for sl in vec![&cs.voters, &cs.voters_outgoing] {
                            for id in sl {
                                if id == &self.rn.raft.id {
                                    found = true;
                                    break;
                                }
                            }
                            if found {
                                break;
                            }
                        }
                        if !found {
                            propc.set_ready(None);
                        }
                    }

                    tokio::select! {
                        _ = self.conf_statec.0.send(cs) => {},
                        _ = donec.changed() => {},
                    }
                },
                _ = self.tickc.1.recv() => {
                    let _ = self.rn.tick().await;
                },
                _ = self.readyc.0.send(rd.clone()) => {
                    self.rn.accept_ready(rd.clone());
                    advancec.set_ready(Some(self.advancec.1.clone()));
                },
                _ = advancec.recv() => {
                    let _ = self.rn.advance(rd.clone()).await;
                    rd = crate::raw_node::Ready::default();
                    advancec.set_ready(None);
                },
                _ = self.stop.1.recv() => {
                    // let _ = self.done.0.send(());
                    return;
                }
            };
        }
    }

    async fn step_no_wait(&self, ctx: &mut Context, m: Message) -> Result<()> {
        self.step_with_wait_option(ctx, m, false).await
    }

    async fn step_wait(&self, ctx: &mut Context, m: Message) -> Result<()> {
        self.step_with_wait_option(ctx, m, true).await
    }

    async fn get_status(&self) -> Status<'a> {
        let mut donec = self.done.clone();
        let (tx, rx) = async_channel::bounded(1);
        tokio::select! {
            _ = self.status.0.send(tx) => {}
            _ = donec.changed() => {
                return Status::default();
            },
        }
        rx.recv().await.unwrap()
    }

    /// Steps advances the state machine using msgs. The Error::StepCancel will be returned, if any.
    async fn step_with_wait_option(&self, ctx: &mut Context, m: Message, wait: bool) -> Result<()> {
        let mut donec = self.done.clone();
        if m.get_msg_type() == MessageType::MsgProp {
            tokio::select! {
                _ = self.recvc.0.send(m) => {return Ok(());},
                _ = ctx.done() => {
                    return Err(Error::StepCancel);
                }
                _ = donec.changed() => {
                    return Err(Error::RaftStopped);
                }
            }
        }
        let ch = self.propc.0.clone();

        let mut pm = MsgWithResult { m, result: None };
        let (tx, rx) = async_channel::bounded(1);
        if wait {
            pm.result = Some(tx);
        }
        tokio::select! {
            _ = ch.send(pm) => {
                if !wait {
                    return Ok(());
                }
            }
            _ = ctx.done() => {
                return Err(Error::StepCancel);
            }
            _ = donec.changed() => {
                return Err(Error::RaftStopped);
            }
        }
        tokio::select! {
            Ok(r) = rx.recv() => {
                if r.is_err() {
                    return r;
                }
            }
            _ = ctx.done() => {
                return Err(Error::StepCancel);
            }
            _ = donec.changed() => {
                return Err(Error::RaftStopped);
            }
        }

        Ok(())
    }
}

#[allow(unused)]
#[cfg(test)]
mod tests {
    use std::{pin::Pin, sync::Arc, time::Duration};

    use tokio::{
        self,
        sync::{broadcast, oneshot, watch, Mutex},
    };

    #[tokio::test]
    async fn test_tokio_select() {
        let (tx_tick, rx_tick) = async_channel::bounded::<i32>(1);
        let (tx_done, mut rx_done) = watch::channel(());

        let mut donec = rx_done.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            // tx_done.send(());
            drop(tx_done);
        });

        tx_tick.send(1).await;
        let timer = tokio::time::sleep(Duration::from_millis(10));
        tokio::select! {
            val = tx_tick.send(1) => {
                println!("ticked!")
            }
            _ = donec.changed() => { println!("done!!")}
            // v = timer => {println!("tick channel be full: {:?}", v)},
        }

        let mut donec1 = rx_done.clone();
        tokio::select! {
            _ = donec1.changed() => { println!("done!!")}
            // v = timer => {println!("tick channel be full: {:?}", v)},
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("{:?}", rx_done.changed().await);
    }

    #[tokio::test]
    async fn test_oneshot_channel() {
        let (tx, rx) = oneshot::channel::<i32>();
        tokio::spawn(async move {
            tx.send(1).unwrap();
        });
        let r = rx.await;
        assert_eq!(r.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_tokio_async_channel() {
        let (s, r) = async_channel::unbounded::<i32>();

        let rclone1 = r.clone();
        tokio::spawn(async move {
            let mut i = rclone1.recv().await;
            println!("receiver1: {:?}", i);
            i = rclone1.recv().await;
            println!("receiver1: {:?}", i);
        });

        let rclone2 = r.clone();
        tokio::spawn(async move {
            let mut i = rclone2.recv().await;
            println!("receiver2: {:?}", i);
            i = rclone2.recv().await;
            println!("receiver2: {:?}", i)
        });

        let _ = s.send(1).await;
        let _ = s.send(2).await;
        let _ = s.send(3).await;
        let _ = s.send(4).await;
        let _ = r.close();
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(s.is_closed());
    }

    #[tokio::test]
    async fn test_close_channel() {
        let (s, r) = watch::channel::<i32>(2);

        let mut r1 = r.clone();
        let _ = tokio::spawn(async move {
            println!("tasks running");
            let out = r1.changed().await;
            println!("task1: {:?}", out)
        });

        let mut r2 = r.clone();
        let _ = tokio::spawn(async move {
            println!("task2 runnning");
            let out = r2.changed().await;
            println!("task2: {:?}", out);
        });

        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(s);
        tokio::time::sleep(Duration::from_secs(3)).await;
        // r
    }
}
