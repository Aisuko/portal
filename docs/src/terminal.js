import * as THREE from 'three';
import { STEPS, SCRIPTS, DESTRUCTIVE_STEP, NODES } from './terminal-script.js';

const COLORS = {
  bg: '#05070a',
  panelLine: '#1c2530',
  text: '#d7dee5',
  muted: '#6b7885',
  accent: '#4ee0a4',
  accent2: '#5fb3ff',
  warn: '#ff6b6b',
  header: '#ffffff',
};

const TYPE_MS_PER_CHAR = 14;
const LINE_PAUSE_MS = 220;
const SCRIPT_END_PAUSE_MS = 2600;

export class TerminalScene {
  constructor(canvas) {
    this.canvas = canvas;
    this.width = 1024;
    this.height = 768;

    this.source = document.createElement('canvas');
    this.source.width = this.width;
    this.source.height = this.height;
    this.ctx = this.source.getContext('2d');

    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));

    this.scene = new THREE.Scene();
    const fov = 32;
    const cameraDistance = 6;
    this.camera = new THREE.PerspectiveCamera(fov, this.width / this.height, 0.1, 100);
    this.camera.position.set(0, 0, cameraDistance);

    this.texture = new THREE.CanvasTexture(this.source);
    this.texture.colorSpace = THREE.SRGBColorSpace;
    this.texture.minFilter = THREE.LinearFilter;

    // Size the plane comfortably inside the camera frustum (with margin for
    // the idle tilt/parallax rotation) so edge content never clips out of view.
    const frustumHeight = 2 * Math.tan(THREE.MathUtils.degToRad(fov / 2)) * cameraDistance;
    const fitMargin = 0.86;
    const planeHeight = frustumHeight * fitMargin;
    const planeWidth = planeHeight * (this.width / this.height);
    const geo = new THREE.PlaneGeometry(planeWidth, planeHeight, 1, 1);
    const mat = new THREE.MeshBasicMaterial({ map: this.texture, transparent: true });
    this.mesh = new THREE.Mesh(geo, mat);
    this.scene.add(this.mesh);

    this.mouse = { x: 0, y: 0 };
    this.targetRot = { x: 0, y: 0 };

    this.activeStepIndex = 1; // "Install k3s HA control plane"
    this.autoplay = true;
    this.status = 'idle';

    this.nodeState = Object.fromEntries(NODES.map((n) => [n, 'idle']));

    this._resetTyping();
    this._bindEvents();
    this._resize();

    this.clock = new THREE.Clock();
    this._raf = requestAnimationFrame(this._tick.bind(this));
  }

  get activeStepName() {
    return STEPS[this.activeStepIndex];
  }

  setActiveStep(name) {
    const idx = STEPS.indexOf(name);
    if (idx === -1) return;
    this.activeStepIndex = idx;
    this._resetTyping();
    this.onStepChange?.(name);
  }

  _resetTyping() {
    this.lines = SCRIPTS[this.activeStepName] || [];
    this.visibleLines = [];
    this.charIndex = 0;
    this.lineIndex = 0;
    this.msAccum = 0;
    this.pauseUntilDone = false;
    this.status = 'running';
    for (const n of NODES) this.nodeState[n] = 'idle';
    this._nodeTouchOrder = 0;
  }

  _bindEvents() {
    window.addEventListener('resize', () => this._resize());
    const parent = this.canvas.parentElement;
    parent.addEventListener('pointermove', (e) => {
      const rect = parent.getBoundingClientRect();
      this.mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
      this.mouse.y = ((e.clientY - rect.top) / rect.height) * 2 - 1;
      this.targetRot.y = this.mouse.x * 0.06;
      this.targetRot.x = -this.mouse.y * 0.04;
    });
    parent.addEventListener('pointerleave', () => {
      this.targetRot.x = 0;
      this.targetRot.y = 0;
    });
  }

  _resize() {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width === 0) return;
    this.renderer.setSize(rect.width, rect.height, false);
    this.camera.aspect = rect.width / rect.height;
    this.camera.updateProjectionMatrix();
  }

  _advanceTyping(dtMs) {
    if (this.lineIndex >= this.lines.length) {
      this.status = 'done';
      this.msAccum += dtMs;
      if (this.autoplay && this.msAccum >= SCRIPT_END_PAUSE_MS) {
        const next = (this.activeStepIndex + 1) % STEPS.length;
        this.activeStepIndex = next;
        this._resetTyping();
        this.onStepChange?.(this.activeStepName);
      }
      return;
    }

    const current = this.lines[this.lineIndex];
    this.msAccum += dtMs;
    const targetChars = Math.floor(this.msAccum / TYPE_MS_PER_CHAR);

    if (targetChars >= current.text.length) {
      if (this.visibleLines.length === this.lineIndex) {
        this.visibleLines.push({ ...current, shown: current.text });
        this._touchNodeState(current.text, current.level);
      }
      if (this.msAccum >= current.text.length * TYPE_MS_PER_CHAR + LINE_PAUSE_MS) {
        this.lineIndex += 1;
        this.msAccum = 0;
      }
    } else {
      const shown = current.text.slice(0, targetChars);
      if (this.visibleLines.length === this.lineIndex) {
        this.visibleLines.push({ ...current, shown });
      } else {
        this.visibleLines[this.lineIndex].shown = shown;
      }
    }
  }

  _touchNodeState(text, level) {
    const match = NODES.find((n) => text.startsWith(n + ':'));
    if (!match) return;
    if (level === 'success') this.nodeState[match] = 'ok';
    else if (level === 'error') this.nodeState[match] = 'warn';
    else this.nodeState[match] = 'busy';
  }

  _tick() {
    const dt = this.clock.getDelta();
    this._advanceTyping(dt * 1000);

    this.mesh.rotation.x += (this.targetRot.x - this.mesh.rotation.x) * 0.06;
    this.mesh.rotation.y += (this.targetRot.y - this.mesh.rotation.y) * 0.06;
    const t = this.clock.elapsedTime;
    this.mesh.rotation.y += Math.sin(t * 0.3) * 0.0025;
    this.mesh.rotation.x += Math.cos(t * 0.25) * 0.0015;

    this._draw();
    this.texture.needsUpdate = true;
    this.renderer.render(this.scene, this.camera);

    this._raf = requestAnimationFrame(this._tick.bind(this));
  }

  _draw() {
    const ctx = this.ctx;
    const w = this.width;
    const h = this.height;

    ctx.fillStyle = COLORS.bg;
    ctx.fillRect(0, 0, w, h);

    // Header bar
    const headerH = 56;
    ctx.strokeStyle = COLORS.panelLine;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(0, headerH);
    ctx.lineTo(w, headerH);
    ctx.stroke();

    ctx.font = '600 20px "JetBrains Mono", monospace';
    ctx.fillStyle = COLORS.muted;
    ctx.textBaseline = 'middle';
    ctx.fillText(this.status, 24, 26);

    ctx.font = '700 22px "JetBrains Mono", monospace';
    ctx.fillStyle = COLORS.header;
    ctx.fillText('cluster-ctl', 24, headerH - 20);

    let nx = 260;
    ctx.font = '16px "JetBrains Mono", monospace';
    for (const node of NODES) {
      const state = this.nodeState[node];
      ctx.fillStyle =
        state === 'ok' ? COLORS.accent : state === 'warn' ? COLORS.warn : state === 'busy' ? COLORS.accent2 : COLORS.muted;
      ctx.beginPath();
      ctx.arc(nx, headerH - 20, 6, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = COLORS.text;
      ctx.fillText(node, nx + 14, headerH - 20);
      nx += 14 + ctx.measureText(node).width + 28;
    }

    // Body split
    const bodyTop = headerH + 1;
    const leftW = Math.round(w * 0.32);
    ctx.beginPath();
    ctx.moveTo(leftW, bodyTop);
    ctx.lineTo(leftW, h - 40);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(0, h - 40);
    ctx.lineTo(w, h - 40);
    ctx.stroke();

    // Steps pane
    ctx.font = '600 14px "JetBrains Mono", monospace';
    ctx.fillStyle = COLORS.muted;
    ctx.fillText('steps', 20, bodyTop + 20);

    let sy = bodyTop + 54;
    const lineH = 30;
    ctx.font = '16px "JetBrains Mono", monospace';
    STEPS.forEach((step, i) => {
      const isActive = i === this.activeStepIndex;
      const isDestructive = step === DESTRUCTIVE_STEP;
      ctx.fillStyle = isActive ? COLORS.header : isDestructive ? COLORS.warn : COLORS.text;
      if (isActive) {
        ctx.fillText('▶', 14, sy);
      }
      ctx.fillText(step, 32, sy);
      sy += lineH;
    });

    // Log pane
    ctx.font = '600 14px "JetBrains Mono", monospace';
    ctx.fillStyle = COLORS.muted;
    ctx.fillText('log', leftW + 20, bodyTop + 20);

    let ly = bodyTop + 54;
    const logLineH = 26;
    ctx.font = '15px "JetBrains Mono", monospace';
    const maxLines = Math.floor((h - 40 - (bodyTop + 54)) / logLineH);
    const visible = this.visibleLines.slice(-maxLines);
    for (const line of visible) {
      const color =
        line.level === 'success' ? COLORS.accent : line.level === 'error' ? COLORS.warn : line.level === 'muted' ? COLORS.muted : COLORS.text;
      ctx.fillStyle = color;
      const prefix = line.level === 'success' ? '✓ ' : line.level === 'error' ? '✗ ' : '  ';
      ctx.fillText(prefix + line.shown, leftW + 20, ly);
      ly += logLineH;
    }

    // Blinking cursor on the last (possibly in-progress) line
    if (this.lineIndex < this.lines.length && Math.floor(this.clock.elapsedTime * 2) % 2 === 0) {
      const last = this.visibleLines[this.visibleLines.length - 1];
      if (last) {
        const prefixW = ctx.measureText('  ').width;
        const textW = ctx.measureText(last.shown).width;
        ctx.fillStyle = COLORS.accent;
        ctx.fillRect(leftW + 20 + prefixW + textW + 2, ly - logLineH - 12, 9, 16);
      }
    }

    // Status bar
    ctx.font = '13px "JetBrains Mono", monospace';
    ctx.fillStyle = COLORS.muted;
    ctx.fillText('↑/↓ select   Enter: run   r: refresh status   PgUp/PgDn: scroll log   q: quit', 16, h - 20);
  }

  dispose() {
    cancelAnimationFrame(this._raf);
    this.renderer.dispose();
  }
}
