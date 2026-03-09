# LP Generic Extra Instances Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Promote the existing extra LP hedger instances into the shared `/lp` operator surface as generic staged entries for Lan without auto-starting them, while keeping Band1/Band2 as the live production pair and keeping `third_lp` hidden until it has a real identity.

**Architecture:** Keep the current shared Fluxboard `/lp` surface and LP API on the shared host. Expand the public operator contract to expose `hype_usdt_lp` and `plume_weth_lp` as staged entries, preserve Chainsaw-compatible IDs/job IDs/Redis/env names, and enroll `service-hedger3` and `service-hedger4` in systemd/Pulse without adding them to `flux-lp.target`. Use readiness gating so staged entries can be viewed and edited but cannot be started, restarted, or enabled until their configs are genuinely ready.

## Progress Tracker

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Task 1 is complete in the worktree. Public LP registry/API now exposes Band1, Band2, `hype_usdt_lp`, and `plume_weth_lp`, keeps `third_lp` hidden from `/api/v1/hedgers/instances`, and adds staged readiness gating for the generic entries. |
| Task 1: Add staged generic LP registry and API contract | completed | main | Added `public_visible`/`staged` hedger metadata, switched the LP API to publish public-visible instances while retaining the full internal registry, and blocked staged restart/enable actions until config readiness passes. Verification: `python3 -m pytest -q --noconftest tests/unit_tests/lp/api/test_app.py tests/unit_tests/lp/test_registry.py` -> `14 passed`. |
| Task 2: Update `/lp` UI for staged generic instances | in_progress | main | Backend payload contract is now available. Next step is red-first Fluxboard coverage for staged generic selector entries, staged/incomplete messaging, and disabled top-level restart/enable controls while config editing stays available. |
| Task 3: Enroll staged generic services without auto-start and refresh docs/contracts | not_started | main | Pending backend/UI contract so the systemd/Pulse/doc changes match the final staged-instance behavior. |
| Task 4: Run LP verification matrix, deploy, and record rollout evidence | not_started | main | Pending implementation of Tasks 1-3. |
