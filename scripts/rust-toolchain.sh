#!/bin/bash

grep '^version' "rust-toolchain.toml" | sed -E 's/version\s*=\s*"([^"]+)"/\1/'
