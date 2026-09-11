// How long a burst of tenants started together takes to answer.
//
// One process polls every port, rather than a curl loop per tenant: a hundred
// shell loops spawning curl every 50ms is two thousand process launches a
// second, which on macOS takes the very cores the tenants are starting on and
// measures the harness instead of the wake.
import { setTimeout as sleep } from "node:timers/promises";

const ports = process.env.EUNHA_DENSITY_PORTS.split(",").map(Number);
const since = Number(process.env.EUNHA_DENSITY_SINCE_MS);
const deadline = Date.now() + 120_000;

const healthy = await Promise.all(ports.map(async (port) => {
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/v1/instance`, { signal: AbortSignal.timeout(2_000) });
      await response.arrayBuffer();
      if (response.ok) return Date.now() - since;
    } catch {
      // Not listening yet.
    }
    await sleep(20);
  }
  return null;
}));

const times = healthy.filter((t) => t !== null).sort((a, b) => a - b);
console.log(JSON.stringify({
  burst: ports.length,
  failed: ports.length - times.length,
  all_healthy_ms: times.at(-1) ?? null,
  p50_ms: times[Math.floor((times.length - 1) / 2)] ?? null,
}));
