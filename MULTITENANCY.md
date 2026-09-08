# Hosted Eunha tenancy plan

## Decision

Eunha hosting should initially keep one Eunha process and one
Mastodon-compatible PostgreSQL database per tenant. The hosting platform may
share physical PostgreSQL and Redis servers, but it must preserve explicit
tenant boundaries.

The first density improvement should be unloading dormant Eunha processes and
starting them on demand. A multi-instance Eunha runtime may be considered
later, but it should still route every tenant to a separate database and Redis
namespace. Adding a `tenant_id` column to Mastodon tables is out of scope.

This ordering preserves Eunha's database-compatibility promise, keeps tenant
exports straightforward, and avoids making every application query part of a
security boundary before measurements show that such complexity is necessary.

## Current measurements

The ten-user benchmark in [BENCHMARKING.md](./BENCHMARKING.md) measured an
Eunha release process at approximately:

- 33 MiB RSS when idle;
- 44–67 MiB RSS under the tested API workloads;
- two useful PostgreSQL connections for a small tenant;
- no meaningful throughput improvement between three and five connections.

An idle process therefore has a real cost, but it is already small. One hundred
continuously resident tenant processes cost roughly 3.2 GiB of application RSS;
one thousand cost roughly 32 GiB. PostgreSQL, Redis data, media processing and
federation load may become limiting factors before process memory at smaller
scales.

## Isolation model

Every hosted tenant retains:

- a separate Mastodon-compatible PostgreSQL database;
- a distinct database role with access only to that database;
- a unique Redis key prefix and tenant-specific Redis credentials;
- tenant-scoped signing keys, secrets and instance configuration;
- an independent migration, backup, restore, export and deletion lifecycle;
- a path to dedicated compute, PostgreSQL or Redis when it becomes large.

Shared infrastructure must never turn a namespace into an authorization
boundary. Redis ACLs enforce the configured key prefix. PostgreSQL roles enforce
database access. The routing layer selects a tenant before any application
authentication or data access occurs.

## Target hosting architecture

```text
Cloudflare custom hostname
          |
          v
Always-on tenant gateway
          |
          +-- running tenant --> Eunha process or runtime shard
          |
          +-- dormant tenant --> single-flight start --> health check --> proxy
                                      |
                                      +--> PostgreSQL database
                                      +--> Redis tenant namespace
                                      +--> shared media storage

Shared scheduler and durable workers
          |
          +--> enqueue or process tenant work
          +--> wake a tenant when application execution is required
```

Cloudflare for SaaS terminates custom hostnames and sends all tenant traffic to
the always-on gateway. The gateway resolves the hostname using control-plane
state and never accepts a tenant identifier supplied only by an untrusted
header or request body.

## Phase 1: isolated active processes

Run one Eunha process per active tenant. For a small instance, begin with:

```toml
[database_pool]
max_connections = 2
min_connections = 0
acquire_timeout_seconds = 5
idle_timeout_seconds = 60
```

The zero minimum allows a quiet process to release every PostgreSQL connection
without stopping. Promote a demonstrably busy tenant to three connections.
Larger pools should follow measurements rather than plan names or member count.

The control plane records at least:

- desired and observed process state;
- runtime host and process identifier;
- last request, streaming connection and background-work activity;
- database and Redis placement;
- migration version and health status;
- resource tier and enforced limits;
- wake, crash and restart history.

## Phase 2: dormancy and on-demand startup

Introduce three process states:

- `running`: normal requests and streaming connections are accepted;
- `draining`: new streaming connections are refused while current requests and
  background work finish;
- `dormant`: no tenant Eunha process is resident; the gateway wakes one when
  necessary.

A tenant is eligible for dormancy only when it has:

- no active HTTP requests;
- no streaming or WebSocket connections;
- no pending in-process work;
- no durable work requiring immediate application execution;
- no scheduled task due within the configured sleep window;
- no migration, backup, restore, provisioning or domain operation in progress.

Start with a conservative idle interval, such as 30–60 minutes. Keep recently
active and higher-tier tenants warm. Allow trial, low-cost or long-dormant
tenants to scale to zero. Dedicated and large instances remain always on.

### Wake behavior

The first request for a dormant tenant performs a single-flight wake:

1. Resolve and authorize the hostname.
2. Acquire a distributed tenant-start lock.
3. Start the assigned Eunha process.
4. Wait for its health endpoint and migration check.
5. Release all requests waiting on that same start operation.
6. Record startup duration and outcome.

Concurrent requests must never create duplicate processes. Wake attempts need a
deadline, bounded request buffering and a clear retryable error when startup
fails.

The gateway may serve carefully selected cached discovery responses while a
tenant sleeps, but cached data must not conceal suspension, deletion or key
rotation. Dynamic APIs and federation inboxes wake the tenant.

Federation request bodies must be bounded and either held until a fast startup
completes or written durably before acknowledgement. The gateway must not
return success for an activity that can be lost.

### Preventing wake abuse

Public actors, WebFinger and inboxes must remain reachable, so authentication
cannot be required before every wake. Mitigations include:

- caching safe public discovery documents at the gateway;
- rate limiting by hostname, route and source reputation;
- coalescing every concurrent wake into one operation;
- applying a minimum warm interval after startup;
- tracking repeated wake-without-use patterns;
- placing abusive or unusually busy tenants on dedicated capacity.

## Phase 3: shared durable workers

Today, work tied to an Eunha process cannot run while that process is dormant.
Before aggressive scale-to-zero, move scheduling and durable job ownership out
of tenant processes.

The shared worker system should:

- persist every job before acknowledging it;
- carry an immutable tenant identity with every job;
- resolve tenant credentials from trusted control-plane state;
- apply per-tenant concurrency and rate limits;
- provide fair scheduling so one tenant cannot starve others;
- retry with bounded backoff and expose dead-letter state;
- wake an Eunha process only for work that cannot execute in the worker;
- record queue latency and execution resource usage by tenant.

Migration jobs bypass transaction poolers and connect directly to PostgreSQL.
They run under an explicit tenant lock and never run as part of ordinary process
startup.

## Phase 4: optional multi-instance runtime shards

Only build a shared Eunha runtime after production measurements show that
resident process overhead is a material host constraint even with dormancy.

A runtime shard may host a bounded group of small tenants:

```text
Eunha runtime shard
  +-- tenant A --> database A --> Redis namespace A
  +-- tenant B --> database B --> Redis namespace B
  +-- tenant C --> database C --> Redis namespace C
```

It must not place multiple tenants in the same Mastodon tables. Each request,
background task, cache entry, metric and outbound operation carries a resolved
tenant context that cannot be replaced by user input.

Begin with a small blast radius, such as 20–50 tenants per shard. Run several
shards per machine. Automatically move noisy or high-value tenants to dedicated
processes.

### Required engineering controls

Before production use, a shared runtime needs:

- hostname-to-tenant routing before authentication and database access;
- tenant-scoped configuration with no mutable global current tenant;
- separate, bounded database pools per tenant or a safe shared pooler;
- tenant-scoped Redis clients and key construction;
- tenant-scoped caches, rate limits, metrics and tracing fields;
- per-tenant CPU, request and background-work fairness;
- bounded HTTP connection pools and outbound federation concurrency;
- tests that deliberately attempt cross-tenant reads and writes;
- shard draining, tenant movement and rollback without DNS changes;
- failure injection proving that one tenant cannot corrupt another;
- memory-growth detection and shard-level admission limits.

A panic, leak or saturated runtime still affects the whole shard. The shard size
is therefore an explicit failure-domain decision, not merely a density setting.

## Rejected design: shared Mastodon tables

Do not add `tenant_id` to Mastodon tables or combine tenants in one modified
Mastodon schema. That design would:

- break direct Mastodon schema compatibility;
- require tenant predicates in every query and unique constraint;
- turn one missed predicate into a cross-tenant disclosure;
- complicate every upstream schema migration;
- make tenant export and restoration more fragile;
- couple large tenants to the storage and migration lifecycle of small ones.

Physical PostgreSQL servers and connection poolers may be shared. Logical
Mastodon databases remain separate.

## PostgreSQL capacity

Host admission must use a global connection budget:

```text
tenant slots = max_connections
             - superuser_reserved_connections
             - reserved_connections
             - operational headroom
```

As an initial example, `max_connections=200` with three superuser-reserved
connections and 17 operational connections leaves 180 tenant slots. That is 90
small, two-connection instances at the absolute boundary. Normal operation
should target 70–80% of that boundary to preserve space for migrations,
failover and tenant movement.

For higher density, use PgBouncer transaction pooling and enforce limits per
database and role. Verify prepared-statement behavior with SQLx before relying
on transaction pooling. Mega instances receive dedicated pools or database
hosts and do not share the small-tenant admission budget.

## Promotion and placement

The control plane should move tenants between modes based on observed behavior:

| Tenant behavior | Placement |
| --- | --- |
| Long-dormant trial or small tenant | Scale-to-zero isolated process |
| Regular small tenant | Warm isolated process |
| Many small tenants after process memory becomes limiting | Bounded runtime shard |
| CPU, federation, media or connection-heavy tenant | Dedicated process |
| Mega instance | Dedicated compute and usually dedicated data services |

Promotion should use sustained CPU, RSS, request latency, database wait time,
connection saturation, Redis usage, federation queue latency and streaming
connections. Member count alone is not a sufficient placement signal.

## Measurement gates

Complete these measurements before moving between phases:

1. Measure cold-start time through the real gateway, including health and
   database checks.
2. Replay federation ingress, retries, media uploads and streaming clients in
   addition to local REST traffic.
3. Measure how many dormant tenants can be woken concurrently without creating
   a PostgreSQL or CPU spike.
4. Compare warm processes with scale-to-zero under representative daily usage.
5. Attribute PostgreSQL, Redis and worker consumption per tenant.
6. Run the same workload against Mastodon where performance claims are needed.
7. Build a shared-runtime prototype only if process RSS remains a meaningful
   capacity constraint after the preceding changes.

The multi-instance runtime is justified when its measured savings exceed its
additional isolation, deployment and incident-response costs. Until then,
isolated processes with zero-minimum database pools and on-demand startup are
the preferred hosting model.
