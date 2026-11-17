#!/usr/bin/env bash

# Recognizes cfg(test) and cfg(all(...)) when test precedes nested predicates,
# rustfmt aligns item attributes and closing braces to bound each exclusion.
is_in_test_module() {
  local file="$1"
  local line_num="$2"

  # States: 1 attribute seen, 2 inside the item header, 3 inside the item body
  awk -v target="$line_num" '
    {
      gated = 0

      if (state == 3) {
        gated = 1
        if ($0 ~ close_re) {
          state = 0
        }
      } else if (state == 2) {
        gated = 1
        if ($0 !~ /^[[:space:]]*\/\//) {
          if ($0 ~ /\{[[:space:]]*(\/\/.*)?$/) {
            state = 3
          } else if ($0 ~ /[;}][[:space:]]*(\/\/.*)?$/) {
            state = 0
          }
        }
      } else if (state == 1) {
        gated = 1
        if ($0 !~ /^[[:space:]]*#\[/ && $0 !~ /^[[:space:]]*\/\//) {
          if ($0 ~ /\{[[:space:]]*(\/\/.*)?$/) {
            state = 3
          } else if ($0 ~ /[;}][[:space:]]*(\/\/.*)?$/) {
            state = 0
          } else {
            state = 2
          }
        }
      } else if ($0 ~ /^[[:space:]]*#\[cfg\((test\)|all\((test[,)]|[^()]*,[[:space:]]*test[,)]))/) {
        gated = 1
        state = 1
        match($0, /^[[:space:]]*/)
        close_re = "^" substr($0, 1, RLENGTH) "\\}[;,]?[[:space:]]*(//.*)?$"
      }

      if (NR == target) {
        inside = gated
      }
    }
    END { exit(inside ? 0 : 1) }
  ' "$file"
}
