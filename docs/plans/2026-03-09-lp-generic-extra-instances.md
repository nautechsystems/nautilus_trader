# LP Generic Extra Instances Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Promote the existing extra LP hedger instances into the shared `/lp` operator surface as generic staged entries for Lan without auto-starting them, while keeping Band1/Band2 as the live production pair and keeping `third_lp` hidden until it has a real identity.

**Architecture:** Keep the current shared Fluxboard `/lp` surface and LP API on the shared host. Expand the public operator contract to expose `hype_usdt_lp` and `plume_weth_lp` as staged entries, preserve Chainsaw-compatible IDs/job IDs/Redis/env names, and enroll `service-hedger3` and `service-hedger4` in systemd/Pulse without adding them to `flux-lp.target`. Use readiness gating so staged entries can be viewed and edited but cannot be started, restarted, or enabled until their configs are genuinely ready.

## Progress Tracker

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Tasks 1-2 are complete in the worktree. The shared `/lp` surface now shows the four public LP entries, preserves generic config editing for staged entries, and visibly blocks restart/enable while staged configs remain incomplete. |
| Task 1: Add staged generic LP registry and API contract | completed | main | Added `public_visible`/`staged` hedger metadata, switched the LP API to publish public-visible instances while retaining the full internal registry, and blocked staged restart/enable actions until config readiness passes. Verification: `python3 -m pytest -q --noconftest tests/unit_tests/lp/api/test_app.py tests/unit_tests/lp/test_registry.py` -> `14 passed`. |
| Task 2: Update `/lp` UI for staged generic instances | completed | main | Updated the shared Hedger surface to treat the selector generically, hide `third_lp`, show staged/incomplete messaging for not-ready generic entries, disable top-level restart/enable controls while leaving config editing available, and stop promising a restart when saving staged configs. Verification: `pnpm --dir fluxboard exec vitest run Hedger.test.tsx main.routes.test.tsx config/uiProfiles.test.ts` -> `41 passed`. |
| Task 3: Enroll staged generic services without auto-start and refresh docs/contracts | in_progress | main | Next step is promoting the staged generic configs into checked-in `.ini` files, enrolling `service-hedger3` and `service-hedger4` in the LP install contract and Pulse, and updating the stack/docs tests so Band1/Band2 stay the only auto-started pair. |
| Task 4: Run LP verification matrix, deploy, and record rollout evidence | not_started | main | Pending implementation of Tasks 1-3. |
