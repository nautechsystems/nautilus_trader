# Codex Agent Instructions

## Superpowers System

Use native skill discovery from `~/.agents/skills` / installed skills.
For this repo, the superpowers workflow skills are vendored under `vendor/superpowers/skills` and should be treated as the repo-local source of truth.

## Deploy Lanes

The shared-host deploy contract is:

- `dev`: mutable canonical repo plus approved worktrees
- `pilot`: pinned release lane for live validation
- `prod`: pinned release lane for approved live trading

Canonical paths for now:

- dev repo: `~/nautilus_trader`
- worktrees: `~/nautilus_trader/.worktrees`
- pilot releases: `~/releases/pilot/<stack>/...`
- prod releases: `~/releases/prod/<stack>/...`

Hard rules:

- never point live services at `~/nautilus_trader`
- never point live services at `.worktrees/*`
- never hot-edit active pilot or prod release roots

Agent command meanings:

- `deploy <stack> to pilot`: create a new pinned pilot release, repoint only pilot, restart only pilot
- `bounce <stack> pilot`: restart only pilot services
- `promote <stack> pilot to prod`: promote the exact tested pilot release into prod and repoint only prod

If live deploy instructions are ambiguous, follow `docs/runbooks/deploy-lanes.md`.

## Skills

A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.

### Available skills

- agent-swarm-lead: Use when running Codex as the lead agent for Agent Swarm and dispatching work to a Codex worker pool. (file: ~/.agents/skills/agent-swarm-lead/SKILL.md)
- brainstorming: You MUST use this before any creative work - creating features, building components, adding functionality, or modifying behavior. Explores user intent, requirements and design before implementation. (file: vendor/superpowers/skills/brainstorming/SKILL.md)
- clean-wd: Use when working directory has mixed changes, multiple features uncommitted, or temp files scattered - organizes all changes into logical atomic commits with doc validation and security checks before optional push (file: ~/chainsaw/.codex/skills/clean-wd/SKILL.md)
- create-ao-issue: Use when creating a planning task issue that must be consumed by Agent Orchestrator for lead/worker delegation, review, and fix loops. (file: ~/.agents/skills/create-ao-issue/SKILL.md)
- dispatching-parallel-agents: Use when facing 2+ independent tasks that can be worked on without shared state or sequential dependencies. (file: vendor/superpowers/skills/dispatching-parallel-agents/SKILL.md)
- dual-repo-wave-closeout: Use when finishing a multi-PR upstream/downstream wave and you need final drift validation, governance updates, and clear done criteria. (file: ~/chainsaw/.codex/skills/dual-repo-wave-closeout/SKILL.md)
- executing-plans: Use when you have a written implementation plan to execute in a separate session with review checkpoints. (file: vendor/superpowers/skills/executing-plans/SKILL.md)
- finishing-a-development-branch: Use when implementation is complete, all tests pass, and you need to decide how to integrate the work - guides completion of development work by presenting structured options for merge, PR, or cleanup. (file: vendor/superpowers/skills/finishing-a-development-branch/SKILL.md)
- receiving-code-review: Use when receiving code review feedback, before implementing suggestions, especially if feedback seems unclear or technically questionable - requires technical rigor and verification, not performative agreement or blind implementation. (file: vendor/superpowers/skills/receiving-code-review/SKILL.md)
- requesting-code-review: Use when completing tasks, implementing major features, or before merging to verify work meets requirements. (file: vendor/superpowers/skills/requesting-code-review/SKILL.md)
- subagent-driven-development: Use when executing implementation plans with independent tasks in the current session. (file: vendor/superpowers/skills/subagent-driven-development/SKILL.md)
- systematic-debugging: Use when encountering any bug, test failure, or unexpected behavior, before proposing fixes. (file: vendor/superpowers/skills/systematic-debugging/SKILL.md)
- test-driven-development: Use when implementing any feature or bugfix, before writing implementation code. (file: vendor/superpowers/skills/test-driven-development/SKILL.md)
- upstream-downstream-sync: Use when syncing a merged chainsaw change into maker-suite with provenance, protected-surface policy, and verification gates. (file: ~/chainsaw/.codex/skills/upstream-downstream-sync/SKILL.md)
- using-git-worktrees: Use when starting feature work that needs isolation from current workspace or before executing implementation plans - creates isolated git worktrees with smart directory selection and safety verification. (file: vendor/superpowers/skills/using-git-worktrees/SKILL.md)
- using-superpowers: Use when starting any conversation - establishes how to find and use skills, requiring skill invocation before any response including clarifying questions. (file: vendor/superpowers/skills/using-superpowers/SKILL.md)
- verification-before-completion: Use when about to claim work is complete, fixed, or passing, before committing or creating PRs - requires running verification commands and confirming output before making any success claims; evidence before assertions always. (file: vendor/superpowers/skills/verification-before-completion/SKILL.md)
- writing-plans: Use when you have a spec or requirements for a multi-step task, before touching code. (file: vendor/superpowers/skills/writing-plans/SKILL.md)
- writing-skills: Use when creating new skills, editing existing skills, or verifying skills work before deployment. (file: vendor/superpowers/skills/writing-skills/SKILL.md)
- skill-creator: Guide for creating effective skills. This skill should be used when users want to create a new skill (or update an existing skill) that extends Codex's capabilities with specialized knowledge, workflows, or tool integrations. (file: ~/.codex/skills/.system/skill-creator/SKILL.md)
- skill-installer: Install Codex skills into `$CODEX_HOME/skills` from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo. (file: ~/.codex/skills/.system/skill-installer/SKILL.md)
- slides: Build, edit, render, import, and export presentation decks with the preloaded `@oai/artifact-tool` JavaScript surface through the artifacts tool. (file: ~/.codex/skills/.system/slides/SKILL.md)
- spreadsheets: Build, edit, recalculate, import, and export spreadsheet workbooks with the preloaded `@oai/artifact-tool` JavaScript surface through the artifacts tool. (file: ~/.codex/skills/.system/spreadsheets/SKILL.md)

### How to use skills

- Discovery: The list above is the skills available in this repo context (name + description + file path). Repo-local paths are relative to the repository root; home-directory paths use `~`. Skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1. After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2. When `SKILL.md` references relative paths (e.g. `scripts/foo.py`), resolve them relative to the skill directory listed above first, and only consider other paths if needed.
  3. If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  4. If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  5. If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue.
