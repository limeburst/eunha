Small-instance benchmark
========================

`scripts/benchmark_small_instance.sh` measures a ten-user Eunha instance with
fresh PostgreSQL data and a namespaced Redis keyspace. It runs a read-heavy mix
of home/public timelines, notifications and account reads, with 5% status
writes. Every pool-size case uses a separate temporary database, which is
removed on exit.

Run a representative burst:

~~~~ sh
mise exec -- env EUNHA_BENCH_SECONDS=30 \
  EUNHA_BENCH_CONCURRENCY=8 \
  EUNHA_BENCH_POOLS="1 2 3 5" \
  scripts/benchmark_small_instance.sh
~~~~

The script requires a local PostgreSQL server, Redis on database 14, Node.js,
and a current `target/release/eunha`. Results are written below
`benchmark-results/`, which is ignored by Git.


2026-09-09 baseline
-------------------

Measured on an Apple Silicon Mac17,2 with 10 CPU cores and 24 GiB RAM, using
PostgreSQL 18 (`shared_buffers=160 MiB`, `work_mem=4 MiB`) and eight concurrent
clients for 20 seconds:

| Pool |  Idle RSS |  Peak RSS | Avg CPU | Peak active DB | Requests/s |      p95 | Errors |
| ---: | --------: | --------: | ------: | -------------: | ---------: | -------: | -----: |
|    1 | 32.64 MiB | 61.81 MiB |  59.52% |              1 |   1,017.95 | 13.87 ms |      0 |
|    2 | 32.16 MiB | 65.23 MiB |  98.03% |              1 |   1,776.00 |  7.19 ms |      0 |
|    3 | 32.70 MiB | 66.09 MiB | 127.40% |              2 |   2,131.05 |  6.42 ms |      0 |
|    5 | 32.78 MiB | 67.45 MiB | 152.64% |              2 |   2,144.55 |  6.74 ms |      0 |

At two concurrent clients, one, two and three connections delivered 758.75,
783.35 and 774.30 requests/s respectively, all without errors. Peak Eunha RSS
was 43.88–45.16 MiB.

This is an intentionally hot local API benchmark, not a federation or media
benchmark. It establishes the per-process baseline and connection-pool knee; a
production sizing exercise must additionally replay real federation ingress,
media uploads, network latency and retained data.


Hosting recommendation
----------------------

For a small, roughly ten-person hosted instance, start with:

~~~~ toml
[database_pool]
max_connections = 2
min_connections = 0
acquire_timeout_seconds = 5
idle_timeout_seconds = 20

[workers]
queue_idle_poll_seconds = 300
~~~~

and `TOKIO_WORKER_THREADS=2` in the process environment.

Two connections were effectively identical to larger pools at ordinary
concurrency and retained most of the throughput at the deliberately excessive
eight-client load. Three is a reasonable promotion tier for a busy tenant;
five showed no material throughput benefit in this run. The idle timeout,
queue poll and worker threads come from the density measurements below, where
they cut an idle tenant's PostgreSQL memory by about five times and its threads
by three.

Budget PostgreSQL slots at the host level instead of multiplying its setting
until every tenant's theoretical pool maximum fits. Reserve slots for
operations and migrations, and enforce an admission budget such as:

~~~~ text
tenant slots = max_connections - superuser_reserved_connections
               - reserved_connections - operational headroom
~~~~

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

Tenants per host
================

The small-instance benchmark measures one process. A hosting service needs the
other number: how many tenants one machine carries. That is mostly decided by
what tenants cost while nobody is using them, which one process under load
cannot show.

`scripts/benchmark_density.sh` answers it. It brings up a PostgreSQL cluster
and Redis server of its own, starts tenants in steps — one eunha process and
one database each, configured as recommended above — and at each step records
the population's idle cost and then its latency under an open-loop request
load spread across every tenant. At the end it restarts a burst of tenants at
once, which is what waking dormant tenants looks like.

~~~~ sh
mise exec -- env EUNHA_DENSITY_COUNTS="10 100 300" \
  EUNHA_DENSITY_LOAD_RPS="200 800" \
  scripts/benchmark_density.sh
~~~~

Every knob is an `EUNHA_DENSITY_*` variable at the top of the script, which
also records the traps that make naive measurements wrong. Two matter when
reading any number here:

 -  **Memory is phys\_footprint, not RSS.** RSS counts the binary's text once
    per process although all of them share it. A tenant's real cost is less
    than half its RSS.
 -  **PostgreSQL connections are costed privately.** Every backend's footprint
    repeats the shared buffer pages it has touched; the script subtracts that
    mapping and adds `shared_buffers` once.


2026-09-11 baseline
-------------------

Measured on the same Mac17,2 (10 cores, 24 GiB), PostgreSQL 18 with
`shared_buffers=1GB`, tenants with a two-connection pool and a 60-second idle
timeout. The machine was also a developer's workstation with most of its
memory already in use, which matters at the top of the range.

What one idle tenant costs, constant from 10 to 300 tenants:

| Resource                             |                  Per idle tenant |
| ------------------------------------ | -------------------------------: |
| eunha footprint, never served load   |                        14–18 MiB |
| eunha footprint after serving load   |                        37–40 MiB |
| Threads                              |                               12 |
| PostgreSQL connections held          |                                2 |
| PostgreSQL private memory            | 9.6 MiB (4.8 MiB per connection) |
| Transactions while idle              |                            3.1/s |
| CPU while idle, eunha and PostgreSQL |                0.09% of one core |
| Empty database on disk               |                         15.3 MiB |

Two of those are not what the tenancy plan assumed. A “zero minimum” pool
never reached zero: the queue loops polled every half-second, so no
connection ever sat idle long enough to close, and an unused tenant held two
PostgreSQL backends indefinitely. And the allocator keeps what a burst grew:
a tenant that has once served load stays at roughly 40 MiB, so capacity has to
be planned on that figure, not on a freshly started process.

Latency under load, spread evenly over every tenant:

| Tenants | 200 req/s p95 | 800 req/s p95 | 1,600 req/s p95 |
| ------: | ------------: | ------------: | --------------: |
|      10 |        6.8 ms |        7.3 ms |          150 ms |
|      50 |        8.0 ms |        7.4 ms |           91 ms |
|     100 |        9.7 ms |        6.5 ms |           35 ms |
|     200 |       10.1 ms |        9.0 ms |           75 ms |
|     300 |       10.0 ms |      4,608 ms |        2,540 ms |

A request costs about 1.95 ms of CPU across eunha and PostgreSQL together, so
800 req/s occupies roughly one and a half of ten cores. The collapse at 300
tenants is therefore not a shortage of CPU: eunha's CPU per request doubled
while most cores stayed idle, which points at scheduling 3,600 threads, or at
memory compression on a machine already short of memory, rather than at work.


Quiet queues
------------

The durable queues — deliveries, inbound activities, media — were polled every
half-second to two seconds whether or not anything could be waiting. Every job
is enqueued by a request the same process serves, so the loops now sleep until
that request wakes them, and otherwise back off to
`[workers] queue_idle_poll_seconds` (30 by default), which only bounds how late
a retry that has come due, or a job another process enqueued, is picked up.

With every other setting unchanged:

| Idle                              |          Before | After |
| --------------------------------- | --------------: | ----: |
| Transactions per tenant           |           3.1/s | 0.2/s |
| eunha CPU, 300 tenants            | 12.1% of a core |  0.8% |
| PostgreSQL CPU, 300 tenants       | 13.6% of a core |  4.8% |
| PostgreSQL connections per tenant |               2 |     2 |

The connections stay because a 30-second poll never lets a 60-second idle
timeout expire. That matters more than the baseline suggested: a connection
costs 4.8 MiB privately when fresh, but **19–24 MiB once it has served the
query mix**, as PostgreSQL keeps catalog caches and prepared statements for
the life of the backend. Two warm connections cost a tenant as much memory as
its eunha process does.

Load behaved as before, and 300 tenants at 800 req/s held a p95 of 8.2 ms. The
baseline's collapse at that point did not recur — nor did it when the baseline
binary was run again (p95 14.5 ms). It was the workstation, not eunha, and the
400-tenant run below shows what the workstation was running out of.


Hosting profile
---------------

Quiet queues stop the polling, but a 60-second idle timeout still outlives the
periodic tasks that run every minute, and every process still starts one Tokio
worker per core. The profile changes configuration only:

~~~~ toml
[database_pool]
idle_timeout_seconds = 20

[workers]
queue_idle_poll_seconds = 300
~~~~

with `TOKIO_WORKER_THREADS=2` in the environment.

| Idle, per tenant                        | Quiet queues | Hosting profile |
| --------------------------------------- | -----------: | --------------: |
| Threads                                 |           12 |               4 |
| PostgreSQL connections held, on average |            2 |            1.35 |
| PostgreSQL private memory               |    38–47 MiB |     about 7 MiB |
| New connections                         |         none |      2 a minute |
| Transactions                            |        0.2/s |           0.2/s |

The memory is the point. Connections are closed before they accumulate caches,
so a sampled connection stays at 4–5 MiB instead of 19–24 MiB. They do not
reach zero: the scheduled-status and poll-expiry tasks both run every minute,
on two connections at once, and sqlx closes idle connections on a 20-second
cycle, so each is open for 20–40 seconds of every minute.

p95 latency at 200 and 800 req/s:

| Tenants |  Quiet queues | Hosting profile |
| ------: | ------------: | --------------: |
|      10 | 7.1 / 15.6 ms |    7.0 / 5.4 ms |
|     100 | 12.2 / 8.6 ms |   10.0 / 5.7 ms |
|     300 | 14.8 / 8.2 ms |  21.8 / 10.6 ms |
|     400 |       not run | 36.9 / 3,934 ms |

Up to 300 tenants the two are indistinguishable within this machine's noise.
At 400 the host collapsed at 800 req/s while decompressing 207,000 pages a
second: the workstation, already using most of its 24 GiB for other work, had
no memory left to keep 400 tenants resident, and every request to one whose
pages had been compressed waited for them. Memory, not CPU or connections, was
the ceiling, and the baseline's earlier collapse at 300 tenants — before the
harness recorded decompression — fits the same explanation.


Waking tenants
--------------

A tenant started alone answers its first request in 145 ms. Started together:

| Burst | All answering |      Median |
| ----: | ------------: | ----------: |
|    10 |   0.45–0.49 s | 0.44–0.49 s |
|    50 |     1.9–2.0 s |   1.8–1.9 s |
|   100 |     3.7–3.8 s |   3.6–3.7 s |

None failed. Scale-to-zero is practical, but a host waking tenants after an
outage or a deploy should admit a bounded number of starts at a time rather
than all of them.


What one Mac mini holds
-----------------------

Memory decides it. Per tenant, from the runs above:

| Tenant                                        |     eunha | PostgreSQL |   Total |
| --------------------------------------------- | --------: | ---------: | ------: |
| Idle, never busy (hosting profile)            | 13–14 MiB |      7 MiB | ~21 MiB |
| Idle after serving load (hosting profile)[^1] |   ~40 MiB |      7 MiB | ~47 MiB |
| Warm, default settings                        |   ~40 MiB |  38–47 MiB | ~85 MiB |

Planning on ~47 MiB — every tenant has at some point been busy — and setting
aside macOS and services (4 GiB), `shared_buffers`, and page cache and Redis:

| Host memory |     Set aside | For tenants | Tenants at 47 MiB | At 85 MiB |
| ----------: | ------------: | ----------: | ----------------: | --------: |
|      16 GiB | 4 + 2 + 2 GiB |       8 GiB |              ~170 |       ~95 |
|      32 GiB | 4 + 4 + 4 GiB |      20 GiB |              ~430 |      ~240 |
|      64 GiB | 4 + 8 + 8 GiB |      44 GiB |              ~950 |      ~530 |

Those are ceilings; plan steady state at 70–80% of them. The other resources
do not bind first at those counts:

 -  **CPU.** A request costs 1.5–2 ms of CPU across eunha and PostgreSQL. Ten
    cores at half utilisation serve roughly 2,500–3,000 req/s, which for 430
    tenants is 6 req/s each at peak — far more than a small instance's clients
    make.
 -  **Connections.** 430 tenants hold about 580 connections idle and up to 860
    when all are busy; `max_connections = 1000` covers it. Beyond roughly 700
    tenants, put PgBouncer in front.
 -  **Threads.** Four per tenant is 1,700 at 430, against a macOS host-wide
    limit of 30,720.
 -  **Disk.** An empty tenant database is 15 MiB. Real ones grow with federated
    content, which this benchmark does not model.

[^1]: The retained eunha heap was measured with a worker per core; two workers
      may retain less, which has not been measured.


Ways to scale further
---------------------

In order of what the measurements say they are worth:

1.  **Let the minute tasks sleep until they are due.** Scheduled statuses and
    poll expiry know when their next item falls due; waking on insert, as the
    queues now do, would let an idle tenant hold no connection at all and stop
    two connection setups a minute per tenant.
2.  **Return retained heap.** A tenant that has served a burst keeps about
    40 MiB where a fresh one needs 14. An allocator that decays unused memory
    back to the system could recover much of the difference, which is over half
    of the planning figure. It needs its own measurement.
3.  **Dormancy.** A dormant tenant costs its database on disk and nothing else,
    and wakes in 145 ms. It needs the gateway and shared workers described in
    [MULTITENANCY.md](./MULTITENANCY.md), and an admission limit on concurrent
    wakes.
4.  **PgBouncer** once connections approach `max_connections`, in transaction
    mode with prepared statements verified against SQLx.
5.  **PostgreSQL on its own machine.** On one Mac mini it competes with tenants
    for the same memory; separating them gives both room, at the cost of a
    network hop on every query, which should be measured.
6.  **More machines,** placing tenants by measured memory rather than by count.


Not yet measured
----------------

 -  **Federation.** Inbound activities (signature verification, fetching remote
    actors) and outbound delivery fan-out are likely the largest CPU cost of a
    well-connected instance, and none of this load includes them.
 -  **Media.** Image decoding and blurhash are CPU-bound and bursty.
 -  **Streaming.** What each open WebSocket costs.
 -  **Real data.** Every database here was empty.
 -  **Dedicated hardware.** Every number was taken on a developer workstation
    already using most of its memory. Run the script on the machine that will
    serve tenants before sizing a fleet from it.
