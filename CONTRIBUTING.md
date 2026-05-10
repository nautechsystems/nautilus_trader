# Contributing to NautilusTrader

We highly value involvement from the trading community, and all contributions are greatly appreciated as they help us continually improve NautilusTrader!

> [!NOTE]
>
> **Integrations:**
> New integrations are a major undertaking for the project and therefore require additional discussion and approval before opening any PRs.
> Please see the [ROADMAP: Community-contributed integrations](ROADMAP.md#community-contributed-integrations) for details on the process
> and [ADAPTERS.md](ADAPTERS.md) for adapter tiers, community listings, and support boundaries.

## Steps

To contribute, follow these steps:

1. Open an issue on GitHub to discuss your proposed changes or enhancements.

2. Once everyone is aligned, fork the `develop` branch and ensure your fork is up-to-date by regularly merging any upstream changes.

3. Set up your development environment by following the [Environment setup guide](docs/developer_guide/environment_setup.md), which covers Rust, Python, and uv. With those prerequisites in place, install the pinned development tools (this includes [prek](https://github.com/j178/prek), which runs pre-commit checks, formatters, and linters before each commit):
    ```bash
    cargo install cargo-binstall --locked  # one-off prerequisite
    make install-tools
    prek install
    ```
   `make install-tools` installs every pinned tool from `Cargo.toml`, `tools.toml`, and `pyproject.toml`. See [Install development tools](docs/developer_guide/environment_setup.md#2-install-development-tools) for what each pinned tool does.

4. Open a pull request (PR) on the `develop` branch with a summary comment and reference to any relevant GitHub issue(s).

5. The CI system will run the full test suite on your code including all unit and integration tests, so include appropriate tests with the PR.

6. Read and understand the Contributor License Agreement (CLA), available at https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md.

7. You will also be required to sign the CLA, which is administered automatically through [CLA Assistant](https://cla-assistant.io/).

8. We will review your code as quickly as possible and provide feedback if any changes are needed before merging.

## Tips

- Follow the established coding practices in the [Developer Guide](https://nautilustrader.io/docs/developer_guide/index.html).
- For documentation changes, follow the style guide in `docs/developer_guide/docs.md` (use sentence case for headings H2 and below).
- Keep PRs small and focused for easier review.
- Reference the relevant GitHub issue(s) in your PR comment.
