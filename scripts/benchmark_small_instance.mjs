import { performance } from "node:perf_hooks";

const base = process.env.EUNHA_BENCH_URL;
const seconds = Number(process.env.EUNHA_BENCH_SECONDS ?? 30);
const concurrency = Number(process.env.EUNHA_BENCH_CONCURRENCY ?? 8);
const tokens = Array.from({ length: 10 }, (_, i) => `eunha-bench-token-${i + 1}`);
const latencies = [];
let requests = 0;
let errors = 0;
let writes = 0;
let sequence = 0;
const deadline = performance.now() + seconds * 1000;

async function request(worker) {
  const n = sequence++;
  const token = tokens[(n + worker) % tokens.length];
  const bucket = n % 20;
  let path = "/api/v1/timelines/home?limit=20";
  let init = {};
  if (bucket < 4) path = "/api/v1/notifications?limit=20";
  else if (bucket < 8) path = "/api/v1/timelines/public?local=true&limit=20";
  else if (bucket < 11) path = "/api/v1/accounts/verify_credentials";
  else if (bucket === 19) {
    path = "/api/v1/statuses";
    init = {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": `bench-${worker}-${n}` },
      body: JSON.stringify({ status: `benchmark status ${worker}-${n}`, visibility: "public" }),
    };
    writes++;
  }
  const start = performance.now();
  try {
    const response = await fetch(base + path, {
      ...init,
      headers: { ...init.headers, authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(10_000),
    });
    await response.arrayBuffer();
    if (!response.ok) errors++;
  } catch {
    errors++;
  } finally {
    latencies.push(performance.now() - start);
    requests++;
  }
}

await Promise.all(Array.from({ length: concurrency }, async (_, worker) => {
  while (performance.now() < deadline) await request(worker);
}));

latencies.sort((a, b) => a - b);
const percentile = (p) => latencies[Math.min(latencies.length - 1, Math.floor(latencies.length * p))] ?? 0;
console.log(JSON.stringify({
  seconds,
  concurrency,
  requests,
  writes,
  errors,
  requests_per_second: requests / seconds,
  latency_ms: { p50: percentile(0.50), p95: percentile(0.95), p99: percentile(0.99), max: latencies.at(-1) ?? 0 },
}));
