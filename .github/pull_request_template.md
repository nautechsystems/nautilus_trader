# Pull Request

**NautilusTrader prioritizes correctness and reliability, please follow existing patterns for validation and testing.**

<!-- PR title: use a capitalized, imperative subject naming the affected surface, for example
     "Fix Bybit post-only rejection flag". Do NOT use Conventional Commits (`feat:`, `fix:`) syntax.
     A squash merge turns this title into the commit subject. -->

- [ ] I have reviewed [CONTRIBUTING.md](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md) and followed the established practices
- [ ] I have not modified `RELEASES.md` (maintainers keep it current to avoid merge conflicts)

## Summary

<!-- Provide a brief description of *what* changed, *why* it was changed, and the impact on the system or users (2-3 sentences). -->

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
- [ ] For PyO3 binding or wrapped Rust doc changes, I ran `make py-stubs-v2` and committed the generated output

## Testing

**Ensure new or changed logic is covered by tests.** Check at least one:

- [ ] Affected code paths are already covered by the test suite
- [ ] I added/updated tests to cover new or changed logic

<!-- Briefly describe how the changes were tested (e.g., unit tests in `tests/unit/test_file.py`, or *additional* manual testing). -->
