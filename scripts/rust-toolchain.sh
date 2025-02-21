#!/bin/bash

awk -F'"' '/version[[:space:]]*=/{gsub(/[[:space:]]/,"",$2); print $2; exit}' rust-toolchain.toml
