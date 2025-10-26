#!/usr/bin/env bash
# Pre-commit hook to run cargo-deny security checks
#
# This hook is optional to avoid forcing all contributors to install cargo-deny,
# but developers who have it installed will benefit from catching security
# advisories, banned crates, and source violations before pushing to CI.
#
# To install cargo-deny: cargo install cargo-deny

set -e

# Check if cargo-deny is installed
if ! command -v cargo-deny &> /dev/null; then
  exit 0
fi

# Run cargo-deny without license checks (matches CI behavior)
# License checks are intentionally excluded as they are verified manually for now
cargo deny --all-features check advisories sources bans
