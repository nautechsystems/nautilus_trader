#!/usr/bin/env bash
# Free disk space on an Ubuntu GitHub Actions runner by removing unused
# SDKs, toolchains, system packages, Docker images, and swap storage.
#
# All categories are opt-in; without flags the script only reports usage.
# Each removal is best-effort: absent paths or packages are silently
# skipped, so the script is safe across evolving runner images.
#
# Usage: free-disk-space.sh [--android] [--dotnet] [--haskell]
#                           [--large-packages] [--docker-images]
#                           [--tool-cache] [--swap-storage] [--extra]

set -euo pipefail

android=0
dotnet=0
haskell=0
large_packages=0
docker_images=0
tool_cache=0
swap_storage=0
extra=0

usage() {
  cat << 'USAGE'
Usage: free-disk-space.sh [options]

All categories are opt-in:
  --android         Remove the Android SDK (/usr/local/lib/android)
  --dotnet          Remove the .NET runtime (/usr/share/dotnet)
  --haskell         Remove GHC and ghcup (/opt/ghc, /usr/local/.ghcup)
  --large-packages  apt-get remove bulky system packages and autoremove
  --docker-images   docker image prune --all --force
  --tool-cache      Remove the actions AGENT_TOOLSDIRECTORY
  --swap-storage    Disable swap and remove /mnt/swapfile
  --extra           Remove further SDKs beyond the categories above
                    (swift, powershell, boost, chromium, chrome,
                    microsoft, julia)
USAGE
}

for arg in "$@"; do
  case "$arg" in
    --android) android=1 ;;
    --dotnet) dotnet=1 ;;
    --haskell) haskell=1 ;;
    --large-packages) large_packages=1 ;;
    --docker-images) docker_images=1 ;;
    --tool-cache) tool_cache=1 ;;
    --swap-storage) swap_storage=1 ;;
    --extra) extra=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "::error::Unknown option: ${arg}" >&2
      usage >&2
      exit 1
      ;;
  esac
done

report_disk() {
  df -h / || true
}

echo "Disk usage before cleanup:"
report_disk

if [ "$android" -eq 1 ]; then
  echo "Removing Android SDK"
  sudo rm -rf /usr/local/lib/android || true
fi

if [ "$dotnet" -eq 1 ]; then
  echo "Removing .NET runtime"
  sudo rm -rf /usr/share/dotnet || true
fi

if [ "$haskell" -eq 1 ]; then
  echo "Removing Haskell toolchain"
  sudo rm -rf /opt/ghc || true
  sudo rm -rf /usr/local/.ghcup || true
fi

# Package names are matched as regexes by apt; on newer runner images many
# of these are already absent, so failures are expected and suppressed.
if [ "$large_packages" -eq 1 ]; then
  echo "Removing large system packages"
  sudo apt-get remove -y '^aspnetcore-.*' || true
  sudo apt-get remove -y '^dotnet-.*' --fix-missing || true
  sudo apt-get remove -y '^llvm-.*' --fix-missing || true
  sudo apt-get remove -y 'php.*' --fix-missing || true
  sudo apt-get remove -y '^mongodb-.*' --fix-missing || true
  sudo apt-get remove -y '^mysql-.*' --fix-missing || true
  sudo apt-get remove -y \
    azure-cli google-chrome-stable firefox powershell mono-devel libgl1-mesa-dri \
    --fix-missing || true
  sudo apt-get remove -y google-cloud-sdk --fix-missing || true
  sudo apt-get remove -y google-cloud-cli --fix-missing || true
  sudo apt-get autoremove -y || true
  sudo apt-get clean || true
fi

if [ "$docker_images" -eq 1 ]; then
  echo "Pruning Docker images"
  if sudo docker info > /dev/null 2>&1; then
    sudo docker image prune --all --force || true
  else
    echo "Docker daemon unavailable; skipping image prune"
  fi
fi

if [ "$tool_cache" -eq 1 ]; then
  echo "Removing actions tool cache"
  sudo rm -rf "${AGENT_TOOLSDIRECTORY:-/opt/hostedtoolcache}" || true
fi

if [ "$swap_storage" -eq 1 ]; then
  echo "Disabling swap storage"
  sudo swapoff -a || true
  sudo rm -f /mnt/swapfile || true
fi

if [ "$extra" -eq 1 ]; then
  echo "Removing additional SDKs"
  for path in \
    /usr/share/swift \
    /usr/local/lib/swift \
    /usr/local/share/powershell \
    /usr/local/share/boost \
    /usr/local/share/chromium \
    /opt/google/chrome \
    /opt/microsoft \
    /usr/local/julia*; do
    sudo rm -rf "$path" || true
  done
fi

echo "Disk usage after cleanup:"
report_disk
