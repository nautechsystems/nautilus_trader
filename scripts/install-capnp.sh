#!/usr/bin/env bash
set -euo pipefail

# Desired version for Linux build (should match what developers use on macOS/etc)
CAPNP_VERSION="1.2.0"

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
  curl --retry 5 --retry-delay 5 -sO https://capnproto.org/capnproto-c++-${CAPNP_VERSION}.tar.gz
  tar zxf capnproto-c++-${CAPNP_VERSION}.tar.gz
  cd capnproto-c++-${CAPNP_VERSION}

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
  echo "Installing Cap'n Proto via Homebrew on macOS..."
  if ! command -v brew &> /dev/null; then
    echo "Error: Homebrew is not installed."
    exit 1
  fi

  # Retry brew install as it can sometimes fail transiently
  MAX_ATTEMPTS=5
  for ((i = 1; i <= MAX_ATTEMPTS; i++)); do
    if brew install capnp; then
      echo "Brew install succeeded."
      break
    fi

    echo "Brew install failed, retrying... (Attempt $i/$MAX_ATTEMPTS)"
    if [ $i -eq $MAX_ATTEMPTS ]; then
      echo "Error: Brew install failed after $MAX_ATTEMPTS attempts."
      exit 1
    fi
    sleep 5
  done

else
  echo "Unsupported OS: ${OS_TYPE}"
  exit 1
fi

echo "Cap'n Proto installed successfully:"
capnp --version
