#!/usr/bin/env bash

set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

# Tauri's Linux build dependencies come from Ubuntu's official repositories.
# Ignore unrelated hosted-runner source fragments and keep mirror failures
# bounded so the workflow can fail with an actionable apt error.
apt_options=(
  -o "Dir::Etc::sourcelist=sources.list"
  -o "Dir::Etc::sourceparts=-"
  -o "Acquire::Retries=2"
  -o "Acquire::http::Timeout=20"
  -o "Acquire::https::Timeout=20"
)

sudo apt-get "${apt_options[@]}" update
sudo apt-get "${apt_options[@]}" install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  xdg-utils
