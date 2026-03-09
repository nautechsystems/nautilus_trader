<!-- DOCID: pulse-ui/readme@v1 -->

# Pulse UI

## Purpose

Describe how to build, test, and serve the Pulse control-plane UI that fronts `/api/pulse/*`.

Pulse UI is a small React + TypeScript + Vite app for service status, logs, and grouped job actions.

## Quick start

Install dependencies from the repository root:

```bash
pnpm --dir pulse-ui install --frozen-lockfile
```

Run the local dev server:

```bash
pnpm --dir pulse-ui dev
```

Build the production bundle:

```bash
pnpm --dir pulse-ui build
```

Preview the built bundle locally:

```bash
pnpm --dir pulse-ui preview
```

Run the test suite:

```bash
pnpm --dir pulse-ui test
```

## Base path behavior

- The default production base path is `/pulse/`.
- `PULSE_UI_BASE_PATH` controls the build-time Vite base path and defaults to `/pulse/`.
- `VITE_PULSE_UI_BASE_PATH` is used by the app’s base-path helpers when tests or custom environments need to override Pulse shell links.
- In dev mode, Vite serves from `/` and proxies `/api/*` to FluxAPI.

## Hosting modes

Pulse is typically served alongside FluxAPI and Fluxboard:

- `python -m flux.runners.tokenmm.run_api --serve-pulse` serves Pulse at `/pulse/*` and Fluxboard at `/tokenmm/*`.
- `python -m flux.runners.equities.run_api --serve-pulse` serves Pulse at `/pulse/*` and Fluxboard at `/equities/*`.

Built bundles default to:

- `pulse-ui/dist` for Pulse assets.
- `fluxboard/dist` for the paired Fluxboard assets when the same runner also serves the shell UI.

## Operational notes

- Pulse reads job state from `/api/pulse/jobs` and related `/api/pulse/*` routes.
- TokenMM and equities shells may both deep-link into Pulse from the same host, so docs and links should keep `/pulse/*`, `/tokenmm/*`, and `/equities/*` distinct.
- Localhost defaults are intentional. Exposing `/api/pulse/*` beyond loopback needs strong network controls.
