#!/usr/bin/env node
import { spawn } from 'node:child_process';

const args = process.argv.slice(2);

if (process.env.FLUXBOARD_E2E !== '1') {
  console.log('[fluxboard] Skipping Playwright E2E tests (set FLUXBOARD_E2E=1 to enable).');
  process.exit(0);
}

const child = spawn('playwright', ['test', ...args], {
  stdio: 'inherit',
  env: process.env,
});

child.on('close', (code) => {
  process.exit(code ?? 0);
});
