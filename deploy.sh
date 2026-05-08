#!/usr/bin/env bash
set -euo pipefail

HOST=100.113.148.66
REMOTE_DIR=~/Git/eunha

ssh -t "$HOST" "
  set -euo pipefail
  cd $REMOTE_DIR
  git pull
  docker compose build
  docker compose up -d
  docker compose ps
"
