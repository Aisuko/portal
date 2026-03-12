# docs

The `portal` project website: an install-process walkthrough built with
Vite, Tailwind CSS v4, and a Three.js-rendered simulation of the
`cluster-ctl` terminal UI. Deployed to GitHub Pages by
[`.github/workflows/deploy-pages.yml`](../.github/workflows/deploy-pages.yml)
on every push to `main` that touches `docs/`.

## Local development

```bash
cd docs
npm install
npm run dev
```

## Production build

```bash
npm run build      # outputs to docs/dist
npm run preview    # serve the built output locally
```

`vite.config.js` reads `VITE_BASE_PATH` for the deployed base path (the CI
workflow sets it to `/portal/`); it defaults to `/` for local dev and
preview.

## Structure

- `index.html` — page markup and sections (hero, architecture, install
  steps, real TUI screenshot, quickstart, docs, footer)
- `src/style.css` — Tailwind entry point + theme tokens
- `src/terminal-script.js` — the simulated log lines per step, adapted from
  the real messages `cluster-ctl` prints (see `cli/src/steps/*.rs`)
- `src/terminal.js` — the Three.js scene: a canvas-textured plane rendering
  the simulated terminal, with a typewriter log animation and mouse parallax
- `src/main.js` — wires the step-selector buttons to the terminal scene
- `public/tui.png` — the real `cluster-ctl` screenshot (copied from
  `references/tui.png`)
