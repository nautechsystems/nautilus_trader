> **Note:** For detailed project plans, strategies, and code architecture, see the `planning/` directory. Specifically, review `planning/plan.md` (strategy and implementation plan) and `planning/engineering.md` (engineering/codebase overview) before starting new code or making architectural decisions.

# Repository Guidelines

This project follows certain conventions for style and tooling.

## Environment Management
- Use [uv](https://docs.astral.sh/uv/) for dependencies.
- Install development dependencies with:
  ```bash
  uv sync --active --all-groups --all-extras
  ```
- Add new packages using:
  ```bash
  uv add <package-name>
  uv sync --active --all-groups --all-extras
  ```
- **Do not** use `pip` directly. All Python packages should be managed through `uv`.

## Code Style
- Follow PEP‑8 along with the [coding standards](docs/developer_guide/coding_standards.md).
- Limit lines to 100 characters and use spaces rather than tabs.
- Write concise comments with one blank line above each block.
- Python docstrings use the imperative mood and NumPy style formatting.

## Development Workflow
- Format code and run pre‑commit checks before committing:
  ```bash
  make format
  make pre-commit
  ```
- Run tests using:
  ```bash
  make pytest        # Python tests
  make cargo-test    # Rust tests
  ```

## Commit Messages
- Use imperative voice and keep subject lines under 60 characters.
- Provide context or links in the body when helpful.

## General Guidance
- Ensure code compiles and tests pass before pushing.
- Keep code intuitive and user-friendly.

