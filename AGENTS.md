# Agent Instructions

Read [AI_POLICY.md](AI_POLICY.md) and [CONTRIBUTING.md](CONTRIBUTING.md) before making changes.
Follow the [coding standards](docs/developer_guide/coding_standards.md) and the relevant developer
guide for the area you change.

## Working rules

**NautilusTrader can execute live trades involving real capital. Hold every change to a very high
standard for correctness, reliability, testing, clarity, and maintainability.**

- Read the affected code and search for existing patterns before proposing or making changes.
- Keep each change focused on the requested outcome. Note unrelated issues instead of fixing them.
- Match the existing style and use established functions, types, names, and dependencies.
- Preserve exact arithmetic for prices, quantities, money, fees, and other discrete values. Use the
  project domain types or `Decimal`.
- Do not add test‑only behavior, branches, attributes, or interfaces to production code.
- Expose the minimum public API and keep the patch focused. Avoid drive‑by refactors, renames, and
  abstractions unrelated to the contribution.
- Change generated artifacts through their source and generator. Never edit them by hand.
- Do not modify `RELEASES.md`. Maintainers keep it current.
- Do not modify `.github/workflows` or `.github/actions` for an external contribution. These paths
  are maintainer‑only.
- Do not use Conventional Commits syntax for commit messages or pull request titles.

## Pull request readiness

**Prepare a complete, review‑ready change before opening a pull request.**

Run the smallest relevant test while developing. Before opening or updating a pull request, run
`make format`, `make pre-commit`, and all tests relevant to the change locally. Record the exact
commands and results in the pull request.

For higher assurance, run `make pre-flight`, which performs the project's broad local validation
suite.

Treat project CI as confirmation of a locally validated change, not as a development loop or a
substitute for local compute. If a maintainer asks for an early draft, agree on its scope before
opening it. After review feedback, batch related fixes and rerun the relevant local checks so the
contributor can push one coherent update.

## Git and public interaction

- Base contributor work on `develop` and target `develop` when preparing a pull request.
- Do not commit, amend, push, or change remote state unless the user explicitly asks.
- Do not open, edit, comment on, review, or otherwise interact with GitHub issues or pull requests
  unless the user explicitly asks. The human contributor controls every public interaction and
  remains responsible for the final communication.
- AI may assist with drafting, but the contributor must understand the text and verify its accuracy.
  When drafting, help preserve the contributor's choices and voice rather than replacing them with
  generic prose.

## Disclosure and attribution

- NautilusTrader does not require disclosure of AI assistance.
- The [AI Policy](AI_POLICY.md) does not override any legal, contractual, or license obligation that
  applies to the contributor or submitted material.
- If the contributor chooses to disclose AI assistance in a commit message, keep the wording
  general, such as `Developed with assistance from AI.`
- The project is neutral among AI labs, vendors, models, and tools, so do not name or promote a
  specific one as attribution in commit messages, pull request titles, or pull request descriptions.
- Do not add an AI tool or model as an author, co‑author, or contributor.
- Do not add `Co-authored-by:` trailers for AI tools or models.
- Do not add branded footers such as `Generated with ...` to commit messages or pull request text.
