# Pulse UI Fluxboard Shell Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restyle `pulse-ui` so it reads as part of the Fluxboard suite while preserving Pulse's existing process-control behavior and API contract.

**Architecture:** Keep Pulse as its own SPA under `/pulse/`, but replace its bespoke shell with Fluxboard-style navigation, color tokens, typography, and panel/table chrome. Reuse the same host for cross-links back to `/tokenmm/*` rather than coupling Pulse to Fluxboard routing.

**Tech Stack:** React 18, Vite, TypeScript, existing Pulse UI components/tests, Fluxboard design tokens as visual reference.

---

### Task 1: Lock the desired shell contract with tests

**Files:**
- Modify: `pulse-ui/src/App.test.tsx`

**Step 1: Write the failing test**

Add a test that expects:
- a Fluxboard-style `flux` brand header
- a visible `Pulse` nav item marked active
- sibling links to `/tokenmm`, `/tokenmm/signal`, `/tokenmm/params`, `/tokenmm/balances`, `/tokenmm/trades`, and `/tokenmm/alerts`

**Step 2: Run test to verify it fails**

Run: `pnpm --dir pulse-ui exec vitest run src/App.test.tsx`

Expected: FAIL because the current Pulse header does not render the Fluxboard-style shell.

### Task 2: Rebuild the Pulse shell with Fluxboard visual language

**Files:**
- Modify: `pulse-ui/src/App.tsx`
- Modify: `pulse-ui/src/components/TopBar.tsx`
- Modify: `pulse-ui/src/index.css`
- Modify if needed: `pulse-ui/src/components/JobGroup.tsx`
- Modify if needed: `pulse-ui/src/components/JobRow.tsx`
- Modify if needed: `pulse-ui/src/components/LogsModal.tsx`

**Step 1: Implement the shell**

Add a top navigation/header that:
- uses Fluxboard-like colors, typography, and spacing
- shows a `flux` brand mark
- renders sibling links back to Fluxboard pages on the same host
- marks `Pulse` as the active destination

**Step 2: Align panel and table styling**

Restyle the content card, banners, group rows, rows, actions, and modal so they visually match Fluxboard's compact operator UI.

**Step 3: Keep behavior unchanged**

Do not change Pulse API calls, grouping logic, action handlers, or logs behavior.

### Task 3: Verify

**Files:**
- Test: `pulse-ui/src/App.test.tsx`
- Test: `pulse-ui/src/api.test.ts`

**Step 1: Run focused tests**

Run: `pnpm --dir pulse-ui exec vitest run src/App.test.tsx src/api.test.ts`

Expected: PASS

**Step 2: Run production build**

Run: `pnpm --dir pulse-ui build`

Expected: successful Vite build
