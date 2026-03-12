# cluster-ctl

A terminal UI for deploying and operating the `portal` k3s/Tailscale
cluster. It talks to nodes directly over SSH (no Ansible/Python runtime
needed) and shells out to `kubectl` only for the one step that needs the
Kubernetes API.

## Build & run

```bash
cd cli
cargo build --release
cargo run --release                # uses ../cluster.toml (repo root)
cargo run --release -- path/to/cluster.toml
CLUSTER_CTL_CONFIG=... cargo run --release
```

Config resolution order: CLI argument, then `$CLUSTER_CTL_CONFIG`, then
the first of `./cluster.toml` / `../cluster.toml` that exists.

## `cluster.toml`

Copy [`cluster.example.toml`](cluster.example.toml) to `cluster.toml`
(gitignored, same as the old `inventory.ini`/`config.yaml`) and fill in
your nodes:

```toml
[[node]]
name = "master-1"
address = "100.100.100.10"        # SSH target / k3s --node-ip
hostname = "master-1.example.ts.net"  # optional: TLS SAN / join hostname
user = "your-user"
roles = ["k3s_master"]            # k3s_master and/or gpu_node
```

The **first** `k3s_master` node in the file is the etcd `--cluster-init`
bootstrap node and the join endpoint of last resort — order matters.

`[settings]` covers the SSH port, k3s channel, Flannel interface, the
kubeconfig path used by "Verify Flannel", and the Rancher install
parameters (hostname, replica count, TLS source, optional chart
versions).

## Authentication

- SSH: tries `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa` against each node
  in turn. On first connect to a host, cluster-ctl records its host key
  (in `$XDG_CONFIG_HOME/cluster-ctl/known_hosts`, TOFU) and verifies it on
  every later connection; a changed key is refused, not silently accepted.
- If no key works, the startup screen's "SSH password" field is used as a
  fallback for every node.
- Every privileged command runs via `sudo`; the startup screen's "sudo
  password" is piped to `sudo -S` over the SSH channel's stdin — it is
  never passed as a command-line argument, so it never shows up in `ps`.
- Both passwords live in memory for the process's lifetime only.
- Leave either field blank to rely purely on keys / `NOPASSWD` sudo.

## Steps

- Deploy all
- Install k3s HA control plane
- Join GPU node(s)
- Configure NVIDIA runtime
- Restart k3s / Flannel recovery
- Install / upgrade Rancher
- Uninstall k3s from all masters
- Update Tailscale on all masters
- Test k3s API reachability
- Verify Flannel networking

"Uninstall" requires typing `UNINSTALL` to confirm — it deletes k3s and
all of its state from every master node.

## Keys

`↑`/`↓` or `j`/`k` select a step, `Enter` runs it, `r` re-probes node
reachability (the dots in the header), `PgUp`/`PgDn` scroll the log,
`End` jumps back to following the live tail, `q`/`Esc` quits.

## Known limitations

- Only Debian/Ubuntu nodes are supported (`apt-get`).
- No SSH agent support — only the key files listed above.
- Node order in `cluster.toml` matters: the first `k3s_master` is the
  bootstrap/join-endpoint-of-last-resort node.
