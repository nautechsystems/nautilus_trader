# Contributing to NautilusTrader

Involvement from the trading community is a goal for this project. All developers
are welcome to open issues/discussions on GitHub for proposed enhancements, feature
requests, and bug reports. Its best practice to keep all discussions for changes
to the codebase public.

To contribute the following steps are suggested;

- Open an issue on GitHub to discuss your proposal
- Take a fork of the develop branch
- Install and setup pre-commit so that the pre-commit hook will be picked up on
  your local machine. This will automatically run various checks, auto-formatters
  and linting tools. More information can be found here https://pre-commit.com/
- Install the latest version of isort `pip install -U isort` (used in the pre-commit)
- Its recommended you install Redis with the default configuration so that integration
  tests will pass on your machine.
- Before committing it's good practice to run `flake8` and `isort .` yourself
- Open a pull request (PR) on the develop branch with some comments, its suggested to
  also reference any related GitHub issues.
- A heads up that once the PR hits the branch the CI system will run the full test suite over
  your code including all unit, integration, acceptance and performance tests.
  Codacy will also automatically run a code review.
- We will endeavour to review your code expeditiously and merge it into develop!

Please include appropriate tests along with your code. Code coverage tracking
will soon be introduced and its a goal to work towards 100%.

Please conform to the established coding practices and also keep PR's focused.

Thank you for your interest in NautilusTrader!
