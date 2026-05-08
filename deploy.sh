#!/bin/bash
set -e

export PATH="$HOME/.orbstack/bin:$PATH"

git pull
docker compose up -d --build
