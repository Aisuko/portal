// Simulated `cluster-ctl` log output, adapted from the real log lines the
// Rust TUI prints for each step (see cli/src/steps/*.rs). Not live SSH
// output — this is a scripted replay for the demo terminal.

export const STEPS = [
  'Deploy all',
  'Install k3s HA control plane',
  'Join GPU node(s)',
  'Configure NVIDIA runtime',
  'Restart k3s / Flannel recovery',
  'Install / upgrade Rancher',
  'Uninstall k3s from all masters',
  'Update Tailscale on all masters',
  'Test k3s API reachability',
  'Verify Flannel networking',
];

export const DESTRUCTIVE_STEP = 'Uninstall k3s from all masters';

// level: 'info' | 'success' | 'error' | 'muted'
const L = (text, level = 'info') => ({ text, level });

export const SCRIPTS = {
  'Install k3s HA control plane': [
    L('Bootstrapping etcd on master-1 (100.100.100.10)...'),
    L('Waiting for k3s API on 100.100.100.10:6443...'),
    L('cluster-init master is up', 'success'),
    L('joining master-2 via https://100.100.100.10:6443'),
    L('master-2: OK', 'success'),
    L('joining master-3 via https://100.100.100.10:6443'),
    L('master-3: OK', 'success'),
  ],
  'Join GPU node(s)': [
    L('connecting to gpu-1 (100.100.100.20)...'),
    L('gpu-1: authenticated as your-user via key'),
    L('gpu-1: installing k3s-agent...'),
    L('gpu-1: OK', 'success'),
    L('no additional gpu_node entries in cluster config', 'muted'),
  ],
  'Configure NVIDIA runtime': [
    L('gpu-1: installing nvidia-container-toolkit...'),
    L('gpu-1: configuring containerd runtime...'),
    L('gpu-1: restarting k3s-agent...'),
    L('gpu-1: OK', 'success'),
  ],
  'Restart k3s / Flannel recovery': [
    L('master-2: ensuring tailscaled is running...'),
    L('master-2: adding systemd wait on tailscale0...'),
    L('master-2: waiting 30s for the node to settle...'),
    L('master-2: restarting k3s...'),
    L('master-2: flannel.1 interface: present', 'success'),
  ],
  'Install / upgrade Rancher': [
    L('installing cert-manager...'),
    L('installing Rancher...'),
    L('Rancher URL: https://rancher.example.ts.net', 'success'),
    L('Bootstrap password: ************', 'success'),
  ],
  'Uninstall k3s from all masters': [
    L('type UNINSTALL to confirm', 'error'),
    L('master-1: ran k3s-uninstall.sh', 'success'),
    L('master-1: k3s state removed', 'success'),
    L('master-2: ran k3s-uninstall.sh', 'success'),
    L('master-2: k3s state removed', 'success'),
    L('master-3: ran k3s-uninstall.sh', 'success'),
    L('master-3: k3s state removed', 'success'),
  ],
  'Update Tailscale on all masters': [
    L('dispatched `tailscale update` on all masters (not waiting for completion)', 'muted'),
    L('master-1: tailscale update: done', 'success'),
    L('master-2: tailscale update: done', 'success'),
    L('master-3: tailscale update: done', 'success'),
  ],
  'Test k3s API reachability': [
    L('master-1: k3s API: OK', 'success'),
    L('master-2: k3s API: OK', 'success'),
    L('master-3: k3s API: OK', 'success'),
  ],
  'Verify Flannel networking': [
    L('Checking Flannel status on all cluster nodes...'),
    L('=== checking flannel.1 / pod routes ===', 'muted'),
    L('master-1: flannel.1 present, routes OK', 'success'),
    L('master-2: flannel.1 present, routes OK', 'success'),
    L('master-3: flannel.1 present, routes OK', 'success'),
    L('gpu-1: flannel.1 present, routes OK', 'success'),
  ],
};

SCRIPTS['Deploy all'] = [
  L('==> Install k3s HA control plane', 'muted'),
  ...SCRIPTS['Install k3s HA control plane'],
  L('==> Join GPU node(s)', 'muted'),
  ...SCRIPTS['Join GPU node(s)'],
  L('==> Configure NVIDIA runtime', 'muted'),
  ...SCRIPTS['Configure NVIDIA runtime'],
  L('==> Install / upgrade Rancher', 'muted'),
  ...SCRIPTS['Install / upgrade Rancher'],
  L('deploy all: complete', 'success'),
];

export const NODES = ['master-1', 'master-2', 'master-3', 'gpu-1'];
