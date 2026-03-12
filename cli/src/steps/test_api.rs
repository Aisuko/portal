use anyhow::{Context, Result};

use crate::config::Node;
use crate::ssh::SshSession;

use super::common::exec_ok;
use super::{run_parallel, summarize, StepCtx};

/// From every master, curl the bootstrap master's API endpoint over
/// Tailscale to confirm it's reachable.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let masters: Vec<Node> = ctx.config.masters().into_iter().cloned().collect();
    let api_host = ctx.config.cluster_init()?.hostname_or_address().to_string();
    let port = ctx.config.settings.ssh_port;

    let results = run_parallel(masters, |node| {
        let ctx = ctx.clone();
        let api_host = api_host.clone();
        async move {
            let session = SshSession::connect(&node, port, &ctx.creds, &ctx.tx)
                .await
                .with_context(|| format!("connecting to {}", node.name))?;
            let out = exec_ok(&session, &format!("curl -k -m 5 https://{api_host}:6443"), false, None, &ctx.tx).await;
            session.close().await;
            out.map(|_| ())
        }
    })
    .await;

    summarize(results, &ctx.tx, "test-api")
}
