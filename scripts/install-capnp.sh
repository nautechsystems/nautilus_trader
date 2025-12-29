#!/usr/bin/env bash
set -euo pipefail

# Read version from capnp-version file (single source of truth)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CAPNP_VERSION_FILE="$REPO_ROOT/capnp-version"

if [[ -f "$CAPNP_VERSION_FILE" ]]; then
  CAPNP_VERSION=$(cat "$CAPNP_VERSION_FILE" | tr -d '[:space:]')
else
  echo "Error: capnp-version file not found at $CAPNP_VERSION_FILE"
  exit 1
fi

# Detect OS
OS="$(uname -s)"
case "${OS}" in
  Linux*) OS_TYPE=Linux ;;
  Darwin*) OS_TYPE=macOS ;;
  *) OS_TYPE="UNKNOWN:${OS}" ;;
esac

echo "Detected OS: ${OS_TYPE}"

if [[ "${OS_TYPE}" == "Linux" ]]; then
  echo "Installing Cap'n Proto ${CAPNP_VERSION} from source on Linux..."

  # Check if already installed to save time
  if command -v capnp &> /dev/null; then
    INSTALLED_VER=$(capnp --version | cut -d' ' -f4)
    if [[ "$INSTALLED_VER" == "$CAPNP_VERSION" ]]; then
      echo "Cap'n Proto $CAPNP_VERSION is already installed."
      exit 0
    fi
  fi

  # Create a temp directory
  TMP_DIR=$(mktemp -d)
  pushd "$TMP_DIR"

  echo "Downloading Cap'n Proto ${CAPNP_VERSION}..."
  curl --retry 5 --retry-delay 5 -sO "https://capnproto.org/capnproto-c++-${CAPNP_VERSION}.tar.gz"
  tar zxf "capnproto-c++-${CAPNP_VERSION}.tar.gz"
  cd "capnproto-c++-${CAPNP_VERSION}"

  echo "Configuring and building..."
  INSTALL_PREFIX="${CAPNP_PREFIX:-/usr/local}"
  ./configure --prefix="${INSTALL_PREFIX}" --disable-static

  # Get number of cores for make
  if command -v nproc &> /dev/null; then
    CORES=$(nproc)
  else
    CORES=1 # Fallback
  fi

  make -j"${CORES}"

  echo "Installing to ${INSTALL_PREFIX}..."
  if [[ "${INSTALL_PREFIX}" == "/usr/local" ]]; then
    sudo make install
    sudo ldconfig
  else
    make install
  fi

  popd
  rm -rf "$TMP_DIR"

elif [[ "${OS_TYPE}" == "macOS" ]]; then
  echo "Installing Cap'n Proto on macOS..."

  # Check if already installed with correct version
  if command -v capnp &> /dev/null; then
    INSTALLED_VER=$(capnp --version | awk '{print $NF}')
    if [[ "$INSTALLED_VER" == "$CAPNP_VERSION" ]]; then
      echo "Cap'n Proto $CAPNP_VERSION is already installed."
      exit 0
    fi
    echo "Installed version ($INSTALLED_VER) differs from required ($CAPNP_VERSION)"
  fi

  # Try Homebrew first
  if command -v brew &> /dev/null; then
    echo "Trying Homebrew..."
    MAX_ATTEMPTS=3
    for ((i = 1; i <= MAX_ATTEMPTS; i++)); do
      if brew install capnp 2> /dev/null || brew upgrade capnp 2> /dev/null; then
        INSTALLED_VER=$(capnp --version | awk '{print $NF}')
        if [[ "$INSTALLED_VER" == "$CAPNP_VERSION" ]]; then
          echo "Homebrew installed correct version."
          break
        else
          echo "Homebrew version ($INSTALLED_VER) differs from required ($CAPNP_VERSION)"
          echo "Building from source instead..."
          break
        fi
      fi
      echo "Brew install failed, retrying... (Attempt $i/$MAX_ATTEMPTS)"
      sleep 5
    done
  fi

  # Verify version, build from source if needed
  INSTALLED_VER=$(capnp --version 2> /dev/null | awk '{print $NF}' || echo "")
  if [[ "$INSTALLED_VER" != "$CAPNP_VERSION" ]]; then
    echo "Building Cap'n Proto ${CAPNP_VERSION} from source on macOS..."

    TMP_DIR=$(mktemp -d)
    pushd "$TMP_DIR"

    curl --retry 5 --retry-delay 5 -sO "https://capnproto.org/capnproto-c++-${CAPNP_VERSION}.tar.gz"
    tar zxf "capnproto-c++-${CAPNP_VERSION}.tar.gz"
    cd "capnproto-c++-${CAPNP_VERSION}"

    ./configure --prefix=/usr/local --disable-static
    make -j"$(sysctl -n hw.ncpu)"
    sudo make install

    popd
    rm -rf "$TMP_DIR"
  fi

else
  echo "Unsupported OS: ${OS_TYPE}"
  exit 1
fi

echo "Cap'n Proto installed successfully:"
capnp --version
