# Pull Request

**NautilusTrader can execute live trades involving real capital. Pull requests are held to a very
high standard for correctness, reliability, testing, clarity, and maintainability.**

> External contributions must not modify files under `.github/workflows` or `.github/actions`;
> workflow changes are maintainer-only.

<!-- PR title: use a capitalized, imperative subject naming the affected surface, for example
     "Fix Bybit post-only rejection flag". Do NOT use Conventional Commits (`feat:`, `fix:`) syntax.
     A squash merge appends the PR number to this title, so do not add a number such as "(#9999)"
     yourself. Aim to keep the resulting subject at 60 characters or fewer. -->

- [ ] A maintainer agreed on the problem and approach in an issue, or this is a small,
  self-contained fix that does not need prior discussion
- [ ] I have read and followed
  [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md)
  and, if I used AI,
  [AI_POLICY.md](https://github.com/nautechsystems/nautilus_trader/blob/develop/AI_POLICY.md)
- [ ] I understand and can explain every submitted change and all information in this PR description
- [ ] This change is complete, locally validated, and ready for review, or a maintainer requested
  this draft
- [ ] I ran `make format`, then ran `make pre-commit` locally and confirmed it passed, or I
  described an agreed limitation below
- [ ] I ran all relevant tests locally, no tests apply to this change, or I described an agreed
  limitation below
- [ ] I have not modified `RELEASES.md` (maintainers keep it current to avoid merge conflicts)

## Summary

<!-- Provide a brief, accurate description of *what* changed, *why* it was changed, and the impact
     on the system or users (2-3 sentences). Remove generic or bloated prose that could hide
     important details. Do not add branded footers or attribution that names a specific AI lab,
     vendor, tool, or model. -->

## Related issues/PRs

<!-- List any related GitHub issues or PRs (e.g., `Closes #123`, `Related to #456`). -->

## Type of change

<!-- Select all that apply. -->

- [ ] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Improvement (non-breaking)
- [ ] Breaking change (impacts existing behavior)
- [ ] Documentation update
- [ ] Maintenance / chore

## Breaking change details (if applicable)

<!-- If this is a breaking change, describe the impact and any migration steps required for users or developers. -->

## Documentation

- [ ] Documentation changes follow the style guide (`docs/developer_guide/docs.md`)
- [ ] For PyO3 binding or wrapped Rust doc changes, I ran `make py-stubs` and committed the generated output

## Testing

**New or changed logic must be covered by tests.** Select all that apply:

- [ ] Affected code paths are already covered by the test suite
- [ ] I added/updated tests to cover new or changed logic
- [ ] No logic changed (documentation, comments, or metadata only)

<!-- Optional: summarize relevant automated or manual validation when it helps reviewers. Exact
     commands and full output are not required. If a relevant check could not run locally, state why
     and describe the limitation discussed with a maintainer. -->
