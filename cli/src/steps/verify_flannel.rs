use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

use crate::log::{log, LogLevel};

use super::StepCtx;

const NOISE: [&str; 3] = ["profile=legacy", "Creating debugging", "recorded in container"];

/// Port of `infra/verify-flannel.sh`. Purely local: talks to the cluster
/// through `kubectl` and the kubeconfig in `settings.kubeconfig`, no SSH
/// involved (per the decision to keep kubectl for k8s-API diagnostics).
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let kubeconfig = ctx.config.kubeconfig_path();
    let tx = &ctx.tx;
    log(tx, "", LogLevel::Info, "Checking Flannel status on all cluster nodes...".to_string());

    let nodes_out = run_kubectl(&kubeconfig, &["get", "nodes", "-o", "name"])
        .await
        .context("listing cluster nodes (is KUBECONFIG valid and the API reachable?)")?;
    let nodes: Vec<String> = nodes_out
        .lines()
        .filter_map(|l| l.strip_prefix("node/"))
        .map(|s| s.to_string())
        .collect();
    if nodes.is_empty() {
        bail!("kubectl returned no nodes for kubeconfig {}", kubeconfig.display());
    }

    for node in &nodes {
        log(tx, node, LogLevel::Info, "=== checking flannel.1 / pod routes ===".to_string());
        cleanup_debug_pods(&kubeconfig, node).await;

        let debug_target = format!("node/{node}");
        let script = "echo 'Flannel interface:'; ip link show flannel.1 2>&1 | head -n 1; echo; \
                       echo 'Pod network routes:'; ip route | grep '10.42' || true; echo;";
        let result = run_kubectl(
            &kubeconfig,
            &["debug", &debug_target, "--attach=true", "--quiet", "--image=busybox", "--", "sh", "-c", script],
        )
        .await;

        match result {
            Ok(out) => {
                for line in out.lines() {
                    if NOISE.iter().any(|n| line.contains(n)) {
                        continue;
                    }
                    log(tx, node, LogLevel::Stdout, line.to_string());
                }
            }
            Err(e) => log(tx, node, LogLevel::Error, format!("failed to inspect {node}: {e}")),
        }

        cleanup_debug_pods(&kubeconfig, node).await;
    }

    Ok(())
}

async fn cleanup_debug_pods(kubeconfig: &Path, node: &str) {
    let Ok(pods) = run_kubectl(kubeconfig, &["get", "pods", "-o", "name"]).await else {
        return;
    };
    let prefix = format!("pod/node-debugger-{node}-");
    let targets: Vec<&str> = pods.lines().filter(|l| l.starts_with(&prefix)).collect();
    if targets.is_empty() {
        return;
    }
    let kubeconfig_flag = format!("--kubeconfig={}", kubeconfig.display());
    let mut args: Vec<&str> = vec![&kubeconfig_flag, "delete", "--now"];
    args.extend(targets);
    let _ = Command::new("kubectl").args(&args).output().await;
}

async fn run_kubectl(kubeconfig: &Path, args: &[&str]) -> Result<String> {
    let kubeconfig_flag = format!("--kubeconfig={}", kubeconfig.display());
    let mut full_args: Vec<&str> = vec![&kubeconfig_flag];
    full_args.extend_from_slice(args);
    let output = Command::new("kubectl")
        .args(&full_args)
        .output()
        .await
        .context("running kubectl (is it installed and on PATH?)")?;
    if !output.status.success() {
        bail!("kubectl {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
