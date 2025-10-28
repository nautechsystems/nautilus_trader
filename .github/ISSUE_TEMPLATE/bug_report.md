---
name: Bug Report
about: Bug – behavior that contradicts the platform's documented or intended design
labels:
  - bug
---

# Bug Report

Use this template only for issues that fit the **Bug** definition.

| Term                          | Definition |
|-------------------------------|------------|
| **Bug**                       | Behavior that contradicts the platform’s documented or intended design as per code, docs, or specs. (i.e., the implementation is incorrect.) |
| **Expectation&nbsp;mismatch** | Behavior that follows the platform’s documented or intended design but differs from what you expected. (i.e., the design/spec might be the problem.) |
| **Enhancement request**       | A request for new functionality or behavior that is not implied by existing design. (i.e., *“It would be great if the platform could…”*) |

**Note:**

- Submitting this issue automatically applies the `bug` label.
- `bug`-labeled issues are triaged with higher priority because they require corrective implementation work.
- **Expectation mismatches** and design-level concerns should be opened as [Discussions](https://github.com/nautechsystems/nautilus_trader/discussions), or RFCs instead, where they can be validated and discussed to consensus before any work is scheduled.
- The absence of a feature is typically not an expectation mismatch, and should be filed as an enhancement request.

## Confirmation

**Before opening a bug report, please confirm:**

- [ ] I’ve re-read the relevant sections of the documentation.
- [ ] I’ve searched existing issues and discussions to avoid duplicates.
- [ ] I’ve reviewed or skimmed the source code (or examples) to confirm the behavior is not by design.
- [ ] I’ve tested this issue using a recent *development* wheel (`dev` develop or `a` nightly) and can still reproduce it.

Checking a recent development wheel can save time because the issue may already have been fixed.
You can install a development wheel by running:

```bash
pip install -U nautilus_trader --pre --index-url https://packages.nautechsystems.io/simple
```

See the [development-wheels](https://github.com/nautechsystems/nautilus_trader#development-wheels) section for more details.

### Expected Behavior

Add here...

### Actual Behavior

Add here...

### Steps to Reproduce the Problem

1.
2.
3.

### Code Snippets or Logs

<!-- If applicable, provide relevant code snippets, error logs, or stack traces. Use code blocks for clarity. -->

<!-- Consider starting from our Minimal Reproducible Example template: -->
<!-- https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/other/minimal_reproducible_example -->

### Specifications

- OS platform:
- Python version:
- `nautilus_trader` version:
