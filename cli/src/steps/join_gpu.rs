use anyhow::{Context, Result};

use crate::config::Node;
use crate::log::LogLevel;
use crate::shellutil::quote;
use crate::ssh::SshSession;

use super::common::{exec_ok, pick_join_endpoint, read_node_token, ENSURE_CURL};
use super::{run_parallel, summarize, StepCtx};

/// Port of `infra/02_join_gpu_node.yml`: join every configured GPU node as
/// a k3s agent against whichever master is reachable first.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let gpu_nodes: Vec<Node> = ctx.config.gpu_nodes().into_iter().cloned().collect();
    if gpu_nodes.is_empty() {
        crate::log::log(&ctx.tx, "", LogLevel::Info, "no gpu_node entries in cluster config, nothing to join".to_string());
        return Ok(());
    }

    let cluster_init = ctx.config.cluster_init()?.clone();
    let all_masters: Vec<Node> = ctx.config.masters().into_iter().cloned().collect();
    let port = ctx.config.settings.ssh_port;
    let channel = ctx.config.settings.k3s_channel.clone();
    let iface = ctx.config.settings.flannel_iface.clone();

    let token = read_node_token(&cluster_init, port, &ctx.creds, &ctx.tx).await?;

    let results = run_parallel(gpu_nodes, |node| {
        let ctx = ctx.clone();
        let all_masters = all_masters.clone();
        let token = token.clone();
        let channel = channel.clone();
        let iface = iface.clone();
        async move { join_agent(&node, port, &all_masters, &token, &channel, &iface, &ctx).await }
    })
    .await;

    summarize(results, &ctx.tx, "join-gpu")
}

async fn join_agent(node: &Node, port: u16, all_masters: &[Node], token: &str, channel: &str, iface: &str, ctx: &StepCtx) -> Result<()> {
    let endpoint = pick_join_endpoint(all_masters)
        .await
        .with_context(|| format!("no reachable k3s master found on port 6443 from {}", node.name))?;

    let session = SshSession::connect(node, port, &ctx.creds, &ctx.tx).await?;
    let result = join_agent_inner(&session, node, &endpoint, token, channel, iface, ctx).await;
    session.close().await;
    result
}

async fn join_agent_inner(session: &SshSession, node: &Node, endpoint: &str, token: &str, channel: &str, iface: &str, ctx: &StepCtx) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    exec_ok(session, ENSURE_CURL, true, sudo_pw, &ctx.tx).await?;

    let node_name = node.hostname_or_address();
    let node_ip = &node.address;
    let install_exec =
        format!("agent --server https://{endpoint}:6443 --token {token} --node-name {node_name} --node-ip {node_ip} --flannel-iface {iface}");
    let script = format!("curl -sfL https://get.k3s.io | INSTALL_K3S_CHANNEL={channel} INSTALL_K3S_EXEC={} sh -", quote(&install_exec));
    exec_ok(session, &script, true, sudo_pw, &ctx.tx).await?;
    Ok(())
}
