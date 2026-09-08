# Small-instance benchmark

`scripts/benchmark_small_instance.sh` measures a ten-user Eunha instance with
fresh PostgreSQL data and a namespaced Redis keyspace. It runs a read-heavy mix
of home/public timelines, notifications and account reads, with 5% status
writes. Every pool-size case uses a separate temporary database, which is
removed on exit.

Run a representative burst:

```sh
mise exec -- env EUNHA_BENCH_SECONDS=30 \
  EUNHA_BENCH_CONCURRENCY=8 \
  EUNHA_BENCH_POOLS="1 2 3 5" \
  scripts/benchmark_small_instance.sh
```

The script requires a local PostgreSQL server, Redis on database 14, Node.js,
and a current `target/release/eunha`. Results are written below
`benchmark-results/`, which is ignored by Git.

## 2026-09-09 baseline

Measured on an Apple Silicon Mac17,2 with 10 CPU cores and 24 GiB RAM, using
PostgreSQL 18 (`shared_buffers=160 MiB`, `work_mem=4 MiB`) and eight concurrent
clients for 20 seconds:

| Pool | Idle RSS | Peak RSS | Avg CPU | Peak active DB | Requests/s | p95 | Errors |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 32.64 MiB | 61.81 MiB | 59.52% | 1 | 1,017.95 | 13.87 ms | 0 |
| 2 | 32.16 MiB | 65.23 MiB | 98.03% | 1 | 1,776.00 | 7.19 ms | 0 |
| 3 | 32.70 MiB | 66.09 MiB | 127.40% | 2 | 2,131.05 | 6.42 ms | 0 |
| 5 | 32.78 MiB | 67.45 MiB | 152.64% | 2 | 2,144.55 | 6.74 ms | 0 |

At two concurrent clients, one, two and three connections delivered 758.75,
783.35 and 774.30 requests/s respectively, all without errors. Peak Eunha RSS
was 43.88–45.16 MiB.

This is an intentionally hot local API benchmark, not a federation or media
benchmark. It establishes the per-process baseline and connection-pool knee; a
production sizing exercise must additionally replay real federation ingress,
media uploads, network latency and retained data.

## Hosting recommendation

For a small, roughly ten-person hosted instance, start with:

```toml
[database_pool]
max_connections = 2
min_connections = 0
acquire_timeout_seconds = 5
idle_timeout_seconds = 60
```

Two connections were effectively identical to larger pools at ordinary
concurrency and retained most of the throughput at the deliberately excessive
eight-client load. Three is a reasonable promotion tier for a busy tenant;
five showed no material throughput benefit in this run.

Budget PostgreSQL slots at the host level instead of multiplying its setting
until every tenant's theoretical pool maximum fits. Reserve slots for
operations and migrations, and enforce an admission budget such as:

```text
tenant slots = max_connections - superuser_reserved_connections
               - reserved_connections - operational headroom
```

For example, `max_connections=200`, three superuser-reserved slots and 17
operational slots leave 180 tenant slots: 90 two-connection small instances,
before accounting for any dedicated large tenants. Do not plan to run at that
absolute edge; a 70–80% steady-state slot target leaves room for migrations,
failover and load movement.

Raising `max_connections` consumes PostgreSQL shared resources even while
connections are idle. `work_mem` is more dangerous: it applies per query
operation, and parallel workers can multiply it. On a 32 GiB host, 200 is a
conservative first ceiling with modest `work_mem`, measured under the actual
query mix; raising it further should follow load testing and memory telemetry,
not tenant count alone.

For substantially more small tenants, put PgBouncer in transaction pooling
mode between Eunha and PostgreSQL, cap pools per database/user, and verify its
prepared-statement configuration with SQLx. Keep mega instances on dedicated
PostgreSQL pools or hosts so they cannot consume the small-tenant connection
and memory budget.
