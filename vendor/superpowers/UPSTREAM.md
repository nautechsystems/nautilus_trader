# Upstream Provenance

Upstream source:
- Repo: `https://github.com/obra/superpowers`
- Base commit used for this vendor snapshot: `e4a2375`
- Fetched on: `2026-03-09`

Why this is vendored:
- Nautilus Trader wants a stable, repo-owned skill library
- this fork intentionally diverges from upstream on workflow behavior and platform wording
- no nested git remote is kept inside `vendor/superpowers`

Manual refresh procedure:
1. Fetch upstream into an isolated checkout or worktree.
2. Review merged upstream changes affecting:
   - `skills/subagent-driven-development/`
   - `skills/dispatching-parallel-agents/`
   - `skills/writing-plans/`
   - `skills/executing-plans/`
   - `skills/using-superpowers/`
   - `skills/using-git-worktrees/`
   - `skills/writing-skills/`
3. Reapply Nautilus-specific changes:
   - safe parallel task lanes
   - dependency/write-scope tracker contract
   - removal of legacy platform-specific wording from vendored files
4. Copy only the curated skill subset and required support files into `vendor/superpowers/`.
5. Verify:
   - legacy platform-specific wording is absent from `vendor/superpowers`
   - `git diff --check` is clean
   - repo `AGENTS.md` paths still match vendored files
