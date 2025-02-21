#!/bin/bash

grep 'version\s*=\s*"' "rust-toolchain.toml" | head -n 1 | sed -E 's/version\s*=\s*"([^"]+)"/\1/' | tr -d '[:space:]'
