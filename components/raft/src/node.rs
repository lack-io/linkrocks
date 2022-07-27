use async_trait::async_trait;
use tokio_context::context::Context;

use crate::error::Result;

/// Node represents a node in raft cluster.
#[async_trait]
pub trait Node {
    /// tick increments the internal logical clock for the Node by a single tick. Election
    /// timeouts and heartbeat timeouts are in units of ticks.
    async fn tick(self);

    /// campaign causes the Node to transition to candidate state and start campaigning to become leader.
    async fn campaign(&self, &mut ctx: Context) -> Result<()>;

    /// Propose proposes that data be appended to the log. Note that proposals can be lost without
    /// notice, therefore it is user's job to ensure proposal retries.
    async fn propose(&self, &mut ctx: Context, data: &[u8]) -> Result<()>;

    /// ProposeConfChange proposes a configuration change. Like any proposal, the
    /// configuration change may be dropped with or without an error being
    /// retruned. In particular, configuration changes are dropped unless the
    /// leader has certainty that there is no prior unapplied configuration
    /// change in its log.
    async fn proposal_conf_change(&self, &mut ctx: Context) -> Result<()>;
}
