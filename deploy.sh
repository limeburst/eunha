#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

git pull
sudo docker compose build
sudo docker compose up -d
sudo docker compose ps
