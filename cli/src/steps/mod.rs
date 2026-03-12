mod common;
pub mod gpu_runtime;
pub mod install_k3s;
pub mod join_gpu;
pub mod rancher;
pub mod restart_k3s;
pub mod tailscale_update;
pub mod test_api;
pub mod uninstall;
pub mod verify_flannel;

use std::future::Future;
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::config::{Config, Node};
use crate::log::{log, LogLevel, LogSender};
use crate::ssh::Credentials;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    All,
    InstallK3s,
    JoinGpu,
    GpuRuntime,
    RestartK3s,
    Rancher,
    Uninstall,
    TailscaleUpdate,
    TestApi,
    VerifyFlannel,
}

impl Step {
    pub const MENU: [Step; 10] = [
        Step::All,
        Step::InstallK3s,
        Step::JoinGpu,
        Step::GpuRuntime,
        Step::RestartK3s,
        Step::Rancher,
        Step::Uninstall,
        Step::TailscaleUpdate,
        Step::TestApi,
        Step::VerifyFlannel,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Step::All => "Deploy all",
            Step::InstallK3s => "Install k3s HA control plane",
            Step::JoinGpu => "Join GPU node(s)",
            Step::GpuRuntime => "Configure NVIDIA runtime",
            Step::RestartK3s => "Restart k3s / Flannel recovery",
            Step::Rancher => "Install / upgrade Rancher",
            Step::Uninstall => "Uninstall k3s from all masters",
            Step::TailscaleUpdate => "Update Tailscale on all masters",
            Step::TestApi => "Test k3s API reachability",
            Step::VerifyFlannel => "Verify Flannel networking",
        }
    }

    pub fn destructive(&self) -> bool {
        matches!(self, Step::Uninstall)
    }
}

#[derive(Clone)]
pub struct StepCtx {
    pub config: Arc<Config>,
    pub creds: Credentials,
    pub tx: LogSender,
}

pub async fn run(step: Step, ctx: &StepCtx) -> Result<()> {
    match step {
        Step::All => {
            install_k3s::run(ctx).await?;
            join_gpu::run(ctx).await
        }
        Step::InstallK3s => install_k3s::run(ctx).await,
        Step::JoinGpu => join_gpu::run(ctx).await,
        Step::GpuRuntime => gpu_runtime::run(ctx).await,
        Step::RestartK3s => restart_k3s::run(ctx).await,
        Step::Rancher => rancher::run(ctx).await,
        Step::Uninstall => uninstall::run(ctx).await,
        Step::TailscaleUpdate => tailscale_update::run(ctx).await,
        Step::TestApi => test_api::run(ctx).await,
        Step::VerifyFlannel => verify_flannel::run(ctx).await,
    }
}

/// Run `f` against every node in `nodes` concurrently (mirroring Ansible's
/// default per-play parallelism across a `hosts:` group), collecting a
/// per-node result rather than aborting the whole step on the first error.
pub async fn run_parallel<F, Fut>(nodes: Vec<Node>, f: F) -> Vec<(String, Result<()>)>
where
    F: Fn(Node) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let mut set = tokio::task::JoinSet::new();
    for node in nodes {
        let name = node.name.clone();
        let fut = f(node);
        set.spawn(async move { (name, fut.await) });
    }
    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        out.push(res.unwrap_or_else(|e| ("<task>".to_string(), Err(anyhow::anyhow!("task panicked: {e}")))));
    }
    out
}

/// Log a success/error line per node and turn the batch into one `Result`
/// for the step as a whole.
pub fn summarize(results: Vec<(String, Result<()>)>, tx: &LogSender, label: &str) -> Result<()> {
    let mut failed = Vec::new();
    for (name, res) in &results {
        match res {
            Ok(()) => log(tx, name, LogLevel::Success, format!("{label}: OK")),
            Err(e) => {
                log(tx, name, LogLevel::Error, format!("{label} failed: {e:#}"));
                failed.push(name.clone());
            }
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("{label} failed on: {}", failed.join(", "));
    }
}
