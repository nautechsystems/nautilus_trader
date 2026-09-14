#!/usr/bin/env bash

# Convert filesystem paths before adding tool-specific syntax such as wheel extras
native_path() {
  if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    echo 'Expected one nonempty filesystem path' >&2
    return 1
  fi

  local platform
  platform="$(uname -s)" || return
  case "$platform" in
    MINGW* | MSYS* | CYGWIN*)
      if ! command -v cygpath > /dev/null 2>&1; then
        echo 'Native Windows paths require cygpath from Git Bash, MSYS2, or Cygwin' >&2
        return 1
      fi
      cygpath -m -- "$1"
      ;;
    *) printf '%s\n' "$1" ;;
  esac
}
