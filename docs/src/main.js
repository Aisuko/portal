import './style.css';
import { TerminalScene } from './terminal.js';
import { STEPS, DESTRUCTIVE_STEP } from './terminal-script.js';

const canvas = document.getElementById('terminal-canvas');
const scene = new TerminalScene(canvas);

const stepList = document.getElementById('step-list');

function renderStepButtons(activeName) {
  stepList.innerHTML = '';
  STEPS.forEach((step) => {
    const btn = document.createElement('button');
    const isActive = step === activeName;
    const isDestructive = step === DESTRUCTIVE_STEP;
    btn.type = 'button';
    btn.textContent = step;
    btn.className = [
      'text-left px-4 py-3 rounded-md border transition-colors',
      isActive
        ? 'border-[var(--color-accent)]/60 bg-[var(--color-accent)]/10 text-white'
        : 'border-[var(--color-line)] bg-[var(--color-panel)] text-[var(--color-muted)] hover:text-white hover:border-white/20',
      isDestructive && !isActive ? 'text-[var(--color-warn)]/80' : '',
    ].join(' ');
    btn.addEventListener('click', () => {
      scene.autoplay = false;
      scene.setActiveStep(step);
      document.getElementById('terminal').scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
    stepList.appendChild(btn);
  });
}

scene.onStepChange = (name) => renderStepButtons(name);
renderStepButtons(scene.activeStepName);
