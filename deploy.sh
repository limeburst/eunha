#!/bin/bash
set -e

export PATH="$HOME/.orbstack/bin:$PATH"

git pull
git submodule sync
git submodule update --init
DATABASE_URL=postgres://limeburst@localhost/eunha sqlx migrate run
docker compose up -d --build
