// Open-loop load across many tenants: requests arrive at a fixed host-wide rate
// whether or not earlier ones have finished, and each lands on a random tenant.
//
// Latency is measured from when a request was *due*, not from when it was
// sent. A closed loop (the small-instance benchmark) waits for a response
// before sending the next request, so a saturated server slows the load down
// and its latency looks flat; here saturation shows up as latency, which is
// what a tenant's users would see.
import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";

const ports = process.env.EUNHA_DENSITY_PORTS.split(",").map(Number);
const rate = Number(process.env.EUNHA_DENSITY_RPS ?? 50);
const seconds = Number(process.env.EUNHA_DENSITY_SECONDS ?? 30);
const maxInflight = Number(process.env.EUNHA_DENSITY_MAX_INFLIGHT ?? 2000);

// The same mix as benchmark_small_instance.mjs.
const mix = [
  [40, "home", () => ["/api/v1/timelines/home?limit=20"]],
  [20, "notifications", () => ["/api/v1/notifications?limit=20"]],
  [20, "public_local", () => ["/api/v1/timelines/public?local=true&limit=20"]],
  [15, "verify_credentials", () => ["/api/v1/accounts/verify_credentials"]],
  [5, "post", (n) => ["/api/v1/statuses", {
    method: "POST",
    headers: { "content-type": "application/json", "idempotency-key": `density-${process.pid}-${n}` },
    body: JSON.stringify({ status: `density status ${n}`, visibility: "public" }),
  }]],
];
const totalWeight = mix.reduce((sum, [w]) => sum + w, 0);

const stats = Object.fromEntries(mix.map(([, name]) => [name, { latencies: [], errors: 0 }]));
let inflight = 0;
let dropped = 0;
let sequence = 0;
const pending = new Set();

function pick() {
  let r = Math.random() * totalWeight;
  for (const entry of mix) {
    if ((r -= entry[0]) < 0) return entry;
  }
  return mix[0];
}

async function fire(due) {
  const n = sequence++;
  const [, name, build] = pick();
  const [path, init = {}] = build(n);
  const port = ports[Math.floor(Math.random() * ports.length)];
  const token = `eunha-bench-token-${1 + Math.floor(Math.random() * 10)}`;
  const s = stats[name];
  try {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      ...init,
      headers: { ...init.headers, authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(10_000),
    });
    await response.arrayBuffer();
    if (!response.ok) s.errors++;
  } catch {
    s.errors++;
  } finally {
    s.latencies.push(performance.now() - due);
  }
}

const start = performance.now();
const deadline = start + seconds * 1000;
let next = start;
while (next < deadline) {
  const now = performance.now();
  while (next <= now && next < deadline) {
    if (inflight >= maxInflight) {
      dropped++;
    } else {
      inflight++;
      const p = fire(next).finally(() => {
        inflight--;
        pending.delete(p);
      });
      pending.add(p);
    }
    // Exponential inter-arrival times: a Poisson process at `rate`.
    next += (-Math.log(1 - Math.random()) / rate) * 1000;
  }
  await sleep(Math.max(0, Math.min(5, next - performance.now())));
}
await Promise.all(pending);
const elapsed = (performance.now() - start) / 1000;

const percentile = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))] ?? 0;
const summarize = (latencies) => {
  const sorted = [...latencies].sort((a, b) => a - b);
  return {
    count: sorted.length,
    p50: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    p99: percentile(sorted, 0.99),
    max: sorted.at(-1) ?? 0,
  };
};
const all = Object.values(stats).flatMap((s) => s.latencies);
console.log(JSON.stringify({
  tenants: ports.length,
  offered_rps: rate,
  achieved_rps: all.length / elapsed,
  seconds: elapsed,
  errors: Object.values(stats).reduce((sum, s) => sum + s.errors, 0),
  dropped,
  latency_ms: summarize(all),
  by_request: Object.fromEntries(Object.entries(stats).map(([name, s]) => [name, { ...summarize(s.latencies), errors: s.errors }])),
}));
