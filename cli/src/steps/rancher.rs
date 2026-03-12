use anyhow::{Context, Result};

use crate::log::{log, LogLevel};
use crate::shellutil::quote;
use crate::ssh::SshSession;

use super::common::{exec_ok, ENSURE_CURL};
use super::StepCtx;

const KUBECONFIG: &str = "/etc/rancher/k3s/k3s.yaml";

const NODE_AFFINITY_PATCH: &str = r#"{
  "spec": {
    "template": {
      "spec": {
        "affinity": {
          "nodeAffinity": {
            "requiredDuringSchedulingIgnoredDuringExecution": {
              "nodeSelectorTerms": [
                {"matchExpressions": [{"key": "node-role.kubernetes.io/control-plane", "operator": "Exists"}]},
                {"matchExpressions": [{"key": "node-role.kubernetes.io/master", "operator": "Exists"}]}
              ]
            }
          }
        }
      }
    }
  }
}"#;

/// Port of `infra/05_install_rancher.yml`: install cert-manager and
/// Rancher via Helm on the bootstrap master, pinned to control-plane
/// nodes, and print the bootstrap password.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let cluster_init = ctx.config.cluster_init()?.clone();
    let port = ctx.config.settings.ssh_port;

    let session = SshSession::connect(&cluster_init, port, &ctx.creds, &ctx.tx).await?;
    let result = install(&session, ctx, &cluster_init.name).await;
    session.close().await;
    result.with_context(|| format!("installing Rancher on {}", cluster_init.name))
}

async fn install(session: &SshSession, ctx: &StepCtx, node_name: &str) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    let tx = &ctx.tx;
    let s = &ctx.config.settings;

    exec_ok(session, ENSURE_CURL, true, sudo_pw, tx).await?;
    exec_ok(
        session,
        "command -v helm >/dev/null 2>&1 || (curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash)",
        true,
        sudo_pw,
        tx,
    )
    .await?;
    exec_ok(session, &with_kubeconfig("k3s kubectl version --client=true"), true, sudo_pw, tx).await?;

    exec_ok(
        session,
        "helm repo add jetstack https://charts.jetstack.io || true; \
         helm repo add rancher-latest https://releases.rancher.com/server-charts/latest || true; \
         helm repo update",
        true,
        sudo_pw,
        tx,
    )
    .await?;

    log(tx, node_name, LogLevel::Info, "installing cert-manager...".to_string());
    let mut cert_manager_cmd = "helm upgrade --install cert-manager jetstack/cert-manager --namespace cert-manager \
         --create-namespace --set crds.enabled=true"
        .to_string();
    if !s.cert_manager_chart_version.is_empty() {
        cert_manager_cmd.push_str(&format!(" --version {}", quote(&s.cert_manager_chart_version)));
    }
    exec_ok(session, &with_kubeconfig(&cert_manager_cmd), true, sudo_pw, tx).await?;

    exec_ok(
        session,
        &with_kubeconfig(
            "k3s kubectl -n cert-manager rollout status deploy/cert-manager --timeout=300s && \
             k3s kubectl -n cert-manager rollout status deploy/cert-manager-webhook --timeout=300s && \
             k3s kubectl -n cert-manager rollout status deploy/cert-manager-cainjector --timeout=300s",
        ),
        true,
        sudo_pw,
        tx,
    )
    .await?;

    exec_ok(
        session,
        &with_kubeconfig("k3s kubectl create namespace cattle-system --dry-run=client -o yaml | k3s kubectl apply -f -"),
        true,
        sudo_pw,
        tx,
    )
    .await?;

    log(tx, node_name, LogLevel::Info, "installing Rancher...".to_string());
    let mut rancher_cmd = format!(
        "helm upgrade --install rancher rancher-latest/rancher --namespace cattle-system \
         --set hostname={} --set replicas={} --set ingress.tls.source={}",
        quote(&s.rancher_hostname),
        s.rancher_replicas,
        quote(&s.rancher_tls_source),
    );
    if !s.rancher_chart_version.is_empty() {
        rancher_cmd.push_str(&format!(" --version {}", quote(&s.rancher_chart_version)));
    }
    exec_ok(session, &with_kubeconfig(&rancher_cmd), true, sudo_pw, tx).await?;

    let patch_cmd = format!(
        "k3s kubectl -n cattle-system patch deployment rancher --type=merge -p {}",
        quote(NODE_AFFINITY_PATCH)
    );
    exec_ok(session, &with_kubeconfig(&patch_cmd), true, sudo_pw, tx).await?;

    exec_ok(
        session,
        &with_kubeconfig("k3s kubectl -n cattle-system rollout status deploy/rancher --timeout=900s"),
        true,
        sudo_pw,
        tx,
    )
    .await?;

    let pw = exec_ok(
        session,
        &with_kubeconfig(
            "k3s kubectl -n cattle-system get secret bootstrap-secret -o go-template='{{.data.bootstrapPassword}}' | base64 -d",
        ),
        true,
        sudo_pw,
        tx,
    )
    .await?;

    log(tx, node_name, LogLevel::Success, format!("Rancher URL: https://{}", s.rancher_hostname));
    log(tx, node_name, LogLevel::Success, format!("Bootstrap password: {}", pw.stdout.trim()));
    Ok(())
}

fn with_kubeconfig(script: &str) -> String {
    format!("export KUBECONFIG={}; {script}", KUBECONFIG)
}
