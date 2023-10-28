# Contributing to NautilusTrader

Involvement from the trading community is a goal for this project, and all help is welcome!
To contribute, follow these steps:

1. Open an issue on GitHub to discuss your proposed changes or enhancements.

2. Once everyone is on the same page, take a fork of the develop branch (or ensure all upstream changes are merged).

3. Install and set up [pre-commit](https://pre-commit.com/) to ensure that the pre-commit hook is picked up on your local machine. This will automatically run various checks, auto-formatters, and linting tools.

4. It is recommended to install Redis using the default configuration so that integration tests will pass on your machine.

5. Open a pull request (PR) on the `develop` branch with a summary comment.

6. The CI system will run the full test-suite over your code including all unit and integration tests, so include appropriate tests
  with the PR.

7. [Deepsource](https://deepsource.io) will perform an automated code review.
  Fix any issues which cause a failed check, and add the commit to your PR.

8. You will also be required to sign a standard Contributor License Agreement (CLA), which is administered automatically through [CLA Assistant](https://cla-assistant.io/).

9. We will review your code as quickly as possible and may provide feedback on needed changes before merging.

## Tips

- Follow the established coding practices outlined in the [Developer Guide](https://docs.nautilustrader.io/developer_guide/index.html).
- Keep PR's small and focused.
- Reference the related GitHub issue(s) in the PR comment.

Thank you for your interest in NautilusTrader!
