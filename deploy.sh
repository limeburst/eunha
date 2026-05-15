#!/bin/bash
set -e

export PATH="$HOME/.orbstack/bin:$PATH"
export DATABASE_URL=postgres://limeburst@localhost/eunha

git pull
git submodule sync
git submodule update --init
sqlx migrate run
docker compose up -d --build
