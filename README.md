# Binary Patch Exchange (BPX)

BPX is a bandwidth optimization layer for HTTP that sends binary deltas instead of full resources. It tracks client-held versions server‑side and returns the smaller of “diff vs full” per request.

## How It Works

- Server maintains per‑session resource→version state.
- Client sends `X-BPX-Session`, `X-Base-Version`, and `Accept-Diff` on subsequent requests.
- Server looks up the client’s base version, computes a diff to the current content, and returns:
  - `X-Diff-Type: binary-delta` with diff body when diff is worthwhile, or
  - `X-Diff-Type: full` with the complete body otherwise.

## Protocol

- Request headers:
  - `X-BPX-Session`: session identifier
  - `X-Base-Version`: client’s version for the resource
  - `Accept-Diff`: comma‑separated formats client accepts
- Response headers:
  - `X-Resource-Version`: server’s current version id
  - `X-BPX-Session`: session id to use next time
  - `X-Diff-Type`: `full` or `binary-delta`
  - `X-Original-Size`: size in bytes of the full content
  - `X-Diff-Size`: diff size in bytes (when diff)
  - `X-BPX-Cache-TTL`: optional cache hint (seconds)

Format negotiation: this PoC supports `binary-delta` and falls back to `full` if the client doesn’t accept it or when diff isn’t worthwhile.

## Binary Diff Wire Format (v1)

Sequential copy model, no offsets encoded:

```
+--------+--------+----------------+
| Op(1B) | Len(3B)| Data           |
+--------+--------+----------------+
```

- `0x01 COPY(length)`: copy next `length` bytes from base
- `0x02 INSERT(length, data)`: insert `data`
- `0x03 DELETE(length)`: skip `length` bytes from base
- `0x04 END`: end of stream

Example: `{ "name":"Bob" }` → `{ "name":"Robert" }`
```
[COPY, 0x000009] [DELETE, 0x000003] [INSERT, 0x000006, "Robert"] [COPY, 0x000002] [END]
```

## Current Capabilities

- In‑memory sessions with TTL cleanup and per‑resource version tracking.
- In‑memory resource store with version snapshots.
- Diff engine using `similar` + binary wire codec (line‑based source diff, sequential wire ops).
- Negotiation for `binary-delta`; graceful fallback to `full`.

Limitations (PoC):
- Only `binary-delta` is implemented; `json-patch`/`bsdiff` not yet available.
- Example server runs HTTP/1.1; crate is h2‑ready but not demoed over h2.
- Some config limits (session/resource caps, max diff size) are not enforced yet.

## Quickstart

Demo server + Python client:

```bash
# Terminal 1
cargo run --example server

# Terminal 2
python3 examples/client.py
```

Manual curl:

```bash
# Initial fetch
curl -v http://127.0.0.1:3000/api/logs/server

# Trigger updates
curl http://127.0.0.1:3000/demo/update

# Differential fetch
curl -v \
  -H "X-BPX-Session: <id>" \
  -H "X-Base-Version: <version>" \
  -H "Accept-Diff: binary-delta" \
  http://127.0.0.1:3000/api/logs/server
```

## Minimal Integration (Rust)

```rust
use bpx::{BpxServer, BpxConfig, diff::similar::SimilarDiffEngine, state::InMemoryStateManager};
use bpx::server::InMemoryResourceStore;
use std::sync::Arc;

let config = BpxConfig::default();
let state = Arc::new(InMemoryStateManager::new(config.clone()));
let diff = Arc::new(SimilarDiffEngine::new());
let store = Arc::new(InMemoryResourceStore::new());

let server = BpxServer::builder()
    .config(config)
    .state_manager(state)
    .diff_engine(diff)
    .build()?;
// server.handle_request(http_request, store).await?
```

## Why BPX

- Reduce bandwidth by transmitting only deltas for frequently polled resources.
- Lower tail latency on constrained links by shrinking payloads dramatically.
- Preserve HTTP semantics and incremental rollout without bespoke client logic.

## When To Use

- Excellent fit: append-only logs, monitoring dashboards, collaborative docs, timelines/feeds, chat threads, config/state polling.
- Poor fit: one-off fetches, highly random payloads, very small bodies, clients that cannot maintain session state.

## Request/Response Flow

- Initial request: client omits session/version; server returns full body + `X-BPX-Session` and `X-Resource-Version`.
- Subsequent request: client sends `X-BPX-Session`, `X-Base-Version`, and `Accept-Diff`.
- Server computes diff vs current content and returns the smaller of diff vs full.
- Fallback: if versions mismatch, diff isn’t worthwhile, or format not accepted → returns full.

## Negotiation

- Client advertises acceptable formats via `Accept-Diff`.
- Server (PoC) supports only `binary-delta`; otherwise falls back to full.
- Optional stricter behavior (e.g., 406 when no overlap) can be enabled by applications.

## Client Responsibilities

- Persist `X-BPX-Session` and per-resource `X-Resource-Version`.
- Send `Accept-Diff` and the base version on subsequent requests.
- Apply diff to base or replace content when `X-Diff-Type: full`.

## Server Responsibilities

- Track per-session resource versions; clean up expired sessions.
- Decide if diff is worthwhile based on size thresholds.
- Emit BPX headers and maintain per-version snapshots for diffing.

## Caching & Intermediaries

- BPX uses custom end-to-end headers; intermediaries may be unaware of diffs.
- Treat BPX responses as dynamic; cache semantics should be configured per endpoint.

## Demo Benchmarks

From `examples/client.py` against the demo server, latest run stored in `bpx_results/bpx_results_20250906_115949.json`:

- Total requests: 15; Full: 10; Diff: 5
- Bytes (BPX): 63,092; Bytes (no BPX): 199,458; Saved: 136,366 (≈68.4%)
- Scenario breakdown:
  - log_monitoring: 6 requests, 5 diffs, 136,366 bytes saved (~96% per update)
  - metrics_dashboard: 5 requests, 0 diffs (line-based diff not compact enough)
  - collaborative_editing: 4 requests, 0 diffs (single-line JSON; char-level/JSON-aware diff improves this)

Notes: The PoC uses line-based source diffing; adaptive or JSON-aware diffs will yield more diffs on metrics/docs.

## Roadmap

- Additional formats (e.g., JSON Patch) and stricter negotiation.
- HTTP/2 example server and h2-first integration guidance.
- Enforce config limits (session/resource caps, max diff size).
- Metrics/tracing hooks to quantify savings and latencies in production.

## License

MIT License - see [LICENSE](LICENSE) file for details.
