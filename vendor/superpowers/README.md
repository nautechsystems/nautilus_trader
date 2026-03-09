# Vendored Superpowers

This is a curated in-repo fork of `obra/superpowers` for Codex/OpenCode workflows in Nautilus Trader.

Source:
- Upstream repo: `https://github.com/obra/superpowers`
- Upstream base snapshot: `e4a2375` on `origin/main` fetched on `2026-03-09`

Local changes in this vendored copy:
- `subagent-driven-development` now supports safe parallel task lanes via `dispatching-parallel-agents`
- `writing-plans` and `executing-plans` now require a real Progress Tracker and task ownership metadata
- user-facing vendored files use Codex/OpenCode wording and `AGENTS.md`-style local instructions
- only the Codex-relevant skill subset and direct support files are vendored

Update policy:
- This vendored tree does not keep its own upstream git remote
- Upstream syncs are manual
- See [UPSTREAM.md](/home/ubuntu/nautilus_trader/vendor/superpowers/UPSTREAM.md) for the refresh procedure
