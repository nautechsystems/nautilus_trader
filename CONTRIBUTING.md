# Contributing to NautilusTrader

Involvement from the trading community is a goal for this project. All help is welcome!
Developers can open issues on GitHub to discuss proposed enhancements/changes, or
to make bug reports.

It's a best practice to keep all discussions regarding changes to the codebase public.

To contribute, the following steps should be followed;

- Open an issue on GitHub to discuss your proposal.

- Once everyone is on the same page, take a fork of the `develop` branch (or ensure all upstream changes are merged).

- Install and setup pre-commit so that the pre-commit hook will be picked up on
  your local machine. This will automatically run various checks, auto-formatters
  and linting tools. Further information can be found here <https://pre-commit.com/>.

- It's recommended you install Redis using the default configuration, so that integration
  tests will pass on your machine.

- Open a pull request (PR) on the `develop` branch with a summary comment.

- The CI system will run the full test-suite over your code including all unit and integration tests, so please include appropriate tests
  with the PR.

- [Codacy](https://www.codacy.com/) will perform an automated code review. Please
  fix any issues which cause a failed check, and add the commit to your PR.

- You'll also be required to sign a standard Contributor License Agreement (CLA), which is
  administered automatically through CLAassisant.

- We will endeavour to review your code expeditiously, there may be some
  feedback on needed changes before merging.

## Tips

- Conform to the established coding practices, see _Coding Standards_ in the
  [Developer Guide](https://docs.nautilustrader.io/developer_guide/index.html).
- Keep PR's small and focused.
- Reference the related GitHub issue(s) in the PR comment.

Thank you for your interest in NautilusTrader!
