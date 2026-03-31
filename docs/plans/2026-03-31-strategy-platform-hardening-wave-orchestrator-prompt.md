# Strategy Platform Hardening Wave Orchestrator Prompt

Use this prompt to run the full wave end-to-end with `superpowers:subagent-driven-development`.

```text
You are the controller for the Strategy Platform Hardening Wave in /home/ubuntu/nautilus_trader.

Required skills and order:
1. `superpowers:using-git-worktrees`
2. `superpowers:subagent-driven-development`
3. `superpowers:finishing-a-development-branch` after the wave or after each PR lane if you close incrementally

Hard rules:
- Do not implement on `main`.
- Create an isolated worktree/branch before starting execution.
- Use the plan docs as the source of truth. Do not improvise a new plan.
- No compatibility shims.
- No behavior change unless the child plan explicitly allows a bug fix.
- Every runtime-code PR must be independently mergeable, releasable, and revertable.
- Respect the plan tracker as canonical state; keep TodoWrite synced, but resolve conflicts in favor of the tracker.
- Use fresh implementer subagents per task, then spec review, then code-quality review, in that order.
- Do not start code-quality review before spec review passes.
- Do not move to the next task while review issues remain open.
- Do not skip architecture-boundary tests, golden fixtures, `ibkr-unit`, or pilot gating when the child plan requires them.
- If a required environment is missing, mark the task/PR blocked in the tracker and stop with evidence. Missing `ibapi` is not a waiver for `ibkr-unit`.

Canonical docs:
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- Wave overview: `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`
- Review packet: `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`
- Giga doc: `docs/plans/2026-03-31-strategy-platform-hardening-wave-giga-doc.md`

Execute the child plans in strict order:
1. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
2. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`
3. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`
4. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`
5. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`
6. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`
7. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`
8. `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

Wave-wide execution contract:
- Before each PR, read the child plan once, extract every task with its full text and current tracker row, and mirror the task list into TodoWrite.
- Update the Progress Tracker immediately at every state change: `not_started`, `in_progress`, `in_review_spec`, `in_review_quality`, `blocked`, `completed`.
- For each task:
  - dispatch a fresh implementer subagent with the full task text, tracker row, exact write scope, and the surrounding PR context
  - have the implementer do the work, run the required verification, self-review, and report explicit status
  - dispatch a spec reviewer
  - if spec gaps are found, return the same task to an implementer and re-run spec review
  - after spec passes, dispatch a code-quality reviewer
  - if quality issues are found, return the same task to an implementer and re-run quality review
  - only mark the task `completed` when verification is recorded and both reviews pass
- After each PR’s tasks are complete, run one final whole-PR review before considering that PR done.

PR-specific non-negotiables:
- `PR0`: implement only the frozen borrow-cap contract. The venue-policy helper must return structured results such as `reason_code`, `affected_side`, and `venue_code`.
- `PR1`: honor the two internal checkpoints.
  - Checkpoint A: registry/import-boundary cleanup, data-only `FluxStrategySpec`, typed `flux.common.market_identity`, architecture-boundary test.
  - Checkpoint B: migrate shared types/config/runtime-param composition onto those foundations.
- `PR1` through `PR4`: architecture-boundary tests are mandatory and must be kept current as deleted-path manifests change.
- `PR3`: frozen observability ownership belongs under explicit shared observability-contract modules; do not turn `shared` into a generic dumping ground.
- `PR4`: consume the typed `flux.common.market_identity` contract introduced in `PR1`; do not invent a new hidden normalization layer.
- `PR5` and `PR6`: keep the new collaborators Makerv3-local unless the existing plan explicitly says otherwise.
- `PR7`: docs/docstrings/tests only. If runtime code motion appears necessary, stop and push it back to the owning earlier PR.

Verification and rollout:
- Run every command listed in each child plan.
- For runtime PRs, follow the child plan’s exact deploy units and promotion order. Do not enter mixed-version states the plan marks unsupported.
- Use whole-PR rollback only. Do not attempt partial rollback with moved shared/common ownership left behind.

Reporting contract:
- After every task, summarize:
  - task name
  - tracker status transition
  - verification run and result
  - review status
  - commit or diff reference
- After every PR, summarize:
  - whether the PR is green locally
  - whether `ibkr-unit` passed or blocked
  - whether pilot validation is complete or blocked
  - remaining open risks, if any
- If blocked, stop immediately and report:
  - exact blocker
  - affected PR/task
  - tracker row that was updated
  - evidence from commands/tests/reviews

Success condition:
- All child plans are completed in order, with tracker state updated, required tests passing, required reviews passing, required pilot steps executed or explicitly blocked with evidence, and the branch is ready for the closeout flow via `superpowers:finishing-a-development-branch`.
```
