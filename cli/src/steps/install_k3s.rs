use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config::Node;
use crate::log::{log, LogLevel};
use crate::shellutil::quote;
use crate::ssh::SshSession;

use super::common::{exec_ok, pick_join_endpoint, read_node_token, wait_for_port, ENSURE_CURL};
use super::{run_parallel, summarize, StepCtx};

/// Cluster-wide values shared by every master install/join command,
/// bundled so the helper functions below don't need a long parameter
/// list each.
#[derive(Clone)]
struct ClusterParams {
    all_masters: Vec<Node>,
    channel: String,
    iface: String,
    cluster_init_hostname: String,
    cluster_init_ip: String,
}

/// Port of `infra/01_install_k3s_ha.yml`: bootstrap the first master with
/// `--cluster-init`, wait for its API, then join the remaining masters
/// against whichever master is reachable first.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let masters = ctx.config.masters();
    if masters.is_empty() {
        bail!("no k3s_master nodes configured");
    }
    let cluster_init: Node = masters[0].clone();
    let joining: Vec<Node> = masters[1..].iter().map(|n| (*n).clone()).collect();

    let params = ClusterParams {
        all_masters: masters.iter().map(|n| (*n).clone()).collect(),
        channel: ctx.config.settings.k3s_channel.clone(),
        iface: ctx.config.settings.flannel_iface.clone(),
        cluster_init_hostname: cluster_init.hostname_or_address().to_string(),
        cluster_init_ip: cluster_init.address.clone(),
    };
    let port = ctx.config.settings.ssh_port;

    log(&ctx.tx, "", LogLevel::Info, format!("Bootstrapping etcd on {} ({})...", cluster_init.name, cluster_init.address));
    bootstrap(&cluster_init, port, &params, ctx)
        .await
        .with_context(|| format!("bootstrapping {}", cluster_init.name))?;

    log(&ctx.tx, "", LogLevel::Info, format!("Waiting for k3s API on {}:6443...", cluster_init.address));
    if !wait_for_port(&cluster_init.address, 6443, Duration::from_secs(240)).await {
        bail!("k3s API on {} ({}) never became reachable on port 6443", cluster_init.name, cluster_init.address);
    }
    log(&ctx.tx, "", LogLevel::Success, "cluster-init master is up".to_string());

    if joining.is_empty() {
        return Ok(());
    }

    let token = read_node_token(&cluster_init, port, &ctx.creds, &ctx.tx).await?;

    let results = run_parallel(joining, |node| {
        let ctx = ctx.clone();
        let params = params.clone();
        let token = token.clone();
        async move { join_master(&node, port, &params, &token, &ctx).await }
    })
    .await;

    summarize(results, &ctx.tx, "install-k3s")
}

async fn bootstrap(node: &Node, port: u16, params: &ClusterParams, ctx: &StepCtx) -> Result<()> {
    let session = SshSession::connect(node, port, &ctx.creds, &ctx.tx).await?;
    let result = bootstrap_inner(&session, node, params, ctx).await;
    session.close().await;
    result
}

async fn bootstrap_inner(session: &SshSession, node: &Node, params: &ClusterParams, ctx: &StepCtx) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    exec_ok(session, ENSURE_CURL, true, sudo_pw, &ctx.tx).await?;

    let node_ip = &node.address;
    let install_exec = format!(
        "server --cluster-init --node-ip {node_ip} --advertise-address {node_ip} --flannel-iface {} \
         --disable servicelb --tls-san {} --tls-san {}",
        params.iface, params.cluster_init_hostname, params.cluster_init_ip
    );
    let script = format!(
        "curl -sfL https://get.k3s.io | INSTALL_K3S_CHANNEL={} INSTALL_K3S_EXEC={} sh -",
        params.channel,
        quote(&install_exec)
    );
    exec_ok(session, &script, true, sudo_pw, &ctx.tx).await?;
    Ok(())
}

async fn join_master(node: &Node, port: u16, params: &ClusterParams, token: &str, ctx: &StepCtx) -> Result<()> {
    let endpoint = pick_join_endpoint(&params.all_masters)
        .await
        .with_context(|| format!("no reachable k3s master found on port 6443 from {}", node.name))?;

    let session = SshSession::connect(node, port, &ctx.creds, &ctx.tx).await?;
    let result = join_master_inner(&session, node, &endpoint, token, params, ctx).await;
    session.close().await;
    result
}

async fn join_master_inner(session: &SshSession, node: &Node, endpoint: &str, token: &str, params: &ClusterParams, ctx: &StepCtx) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    exec_ok(session, ENSURE_CURL, true, sudo_pw, &ctx.tx).await?;

    let node_ip = &node.address;
    let install_exec = format!(
        "server --server https://{endpoint}:6443 --token {token} --node-ip {node_ip} --advertise-address {node_ip} \
         --flannel-iface {} --disable servicelb --tls-san {} --tls-san {}",
        params.iface, params.cluster_init_hostname, params.cluster_init_ip
    );
    let script = format!(
        "curl -sfL https://get.k3s.io | INSTALL_K3S_CHANNEL={} INSTALL_K3S_EXEC={} sh -",
        params.channel,
        quote(&install_exec)
    );
    exec_ok(session, &script, true, sudo_pw, &ctx.tx).await?;
    Ok(())
}
