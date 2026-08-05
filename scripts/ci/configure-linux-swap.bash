#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
  echo "Skipping Linux swap configuration on a non-Linux host"
  exit 0
fi

swap_file=/swapfile
swap_size="${1:-8G}"

sudo swapoff -a || true
sudo rm -f /mnt/swapfile "$swap_file" || true
sudo fallocate -l "$swap_size" "$swap_file"
sudo chmod 600 "$swap_file"
sudo mkswap "$swap_file"
sudo swapon "$swap_file"

free -h
swapon --show
