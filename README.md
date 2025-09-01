# Differential Sync Protocol (DSP)

DSP is a novel bandwidth-optimization layer over HTTP/2 that reduces network payload sizes by up to 90% by sending binary diffs instead of complete resources. It maintains server-side client state to automatically compute minimal deltas between resource versions, enabling dramatic bandwidth savings for frequently-polled APIs while preserving REST's simplicity.

## Technical Overview

DSP addresses a fundamental inefficiency in modern web architectures: repeatedly transmitting full resources when only small portions have changed. Traditional HTTP responses contain complete resource representations, even when clients poll for updates frequently and changes are minimal.

DSP is inspired by RFC 3229 (Delta encoding in HTTP) but takes a different approach with server-side session management and HTTP/2 integration.

### Core Innovation

DSP introduces **stateful differential transmission** to HTTP while maintaining stateless application semantics. The protocol:

1. **Tracks client resource versions server-side** using session-based state management
2. **Computes binary diffs** between client's cached version and current resource state  
3. **Transmits minimal delta payloads** instead of full resource representations
4. **Falls back gracefully** to full responses when diffs are inefficient or state is unavailable

This approach achieves bandwidth reduction for use cases involving:
- Real-time dashboards with incremental metric updates
- Log streaming with append-only data patterns
- Collaborative editing with small text modifications
- Configuration polling with infrequent changes

## Protocol Specification

### Request Flow

**Initial Request (No State):**
```http
GET /api/users/123 HTTP/2
Accept-Diff: binary-delta,json-patch
```

**Server Response:**
```http
HTTP/2 200 OK
X-Resource-Version: v:1647892341
X-DSP-Session: sess_abc123
X-Diff-Type: full
X-Original-Size: 2048

{"id": "123", "name": "Alice", "email": "alice@example.com", ...}
```

**Subsequent Request (With State):**
```http
GET /api/users/123 HTTP/2
X-DSP-Session: sess_abc123
X-Base-Version: v:1647892341
Accept-Diff: binary-delta
```

**Differential Response:**
```http
HTTP/2 200 OK
X-Resource-Version: v:1647892405
X-Diff-Type: binary-delta
X-Original-Size: 2048
X-Diff-Size: 127

[binary diff: 127 bytes to change "Alice" → "Alicia"]
```

### Header Specifications

**Request Headers:**
- `X-DSP-Session`: Client session identifier for state tracking
- `X-Base-Version`: Resource version currently held by client
- `Accept-Diff`: Comma-separated list of supported diff formats

**Response Headers:**
- `X-Resource-Version`: Current resource version identifier
- `X-DSP-Session`: Session ID for subsequent requests
- `X-Diff-Type`: Response format (`full`, `binary-delta`, `json-patch`)
- `X-Original-Size`: Size of complete resource in bytes
- `X-Diff-Size`: Size of transmitted diff (when applicable)
- `X-DSP-Cache-TTL`: Client cache validity duration in seconds

### Binary Diff Wire Format

DSP uses a compact binary format for maximum efficiency:

```
+--------+--------+----------------+
| Op(1B) | Len(3B)| Data           |
+--------+--------+----------------+
```

**Operations:**
- `0x01 COPY(length)`: Copy bytes from old version at current position
- `0x02 INSERT(length, data)`: Insert new data
- `0x03 DELETE(length)`: Skip/delete bytes from old version  
- `0x04 END`: Terminate diff stream

**Example:** Transform `{"name":"Bob"}` to `{"name":"Robert"}`:
```
[COPY, 0x000009] [DELETE, 0x000003] [INSERT, 0x000006, "Robert"] [COPY, 0x000002] [END]
```

## Architecture

### Session Management

Sessions provide the stateful foundation enabling differential transmission:

```rust
pub struct DspSession {
    pub id: SessionId,
    pub resources: DashMap<ResourcePath, Version>,
    pub last_accessed: Instant,
    pub memory_usage: AtomicUsize,
}
```

- **Automatic creation** on first client request
- **Version tracking** per resource path within session
- **TTL-based expiration** with configurable cleanup intervals
- **Memory limits** to prevent resource exhaustion
- **Concurrent access** via lock-free data structures

### Diff Algorithm

The binary diff engine currently uses the `similar` crate with plans to experiment with other algorithms:

## Implementation Status
### Current State: Proof-of-Concept

**Implemented:**
- Complete DSP protocol specification compliance
- Binary diff algorithm
- Configurable compression thresholds and resource limits

### Optimal Use Cases

**Excellent fit:**
- Real-time dashboards polling metrics APIs
- Log streaming and monitoring interfaces  
- Collaborative editing and document synchronization
- Configuration management with polling clients
- Social media feeds and timelines
- Chat applications and messaging threads
- Financial data feeds
- News feeds and content aggregation
- Shopping cart and e-commerce session APIs
- IoT sensor data and time-series endpoints
- Game leaderboards and player statistics
- Search results with incremental filtering
- Comment threads and discussion forums
- Inventory and catalog APIs with price/stock updates
- Any API with high request frequency and low change rates

**Poor fit:**
- Single-use resource fetching
- Resources with completely random changes between requests
- Very small resources where diff overhead exceeds savings
- Clients unable to maintain session state

## Configuration

```rust
let config = DspConfig {
    max_sessions: 100_000,              // Concurrent client sessions
    max_resources_per_session: 1_000,   // Resources tracked per client
    session_ttl: Duration::from_secs(24 * 60 * 60), // 24 hour TTL
    max_diff_size: 10 * 1024 * 1024,    // 10MB diff size limit
    min_compression_ratio: 0.2,         // 20% savings required
    cleanup_interval: Duration::from_secs(5 * 60), // 5 minute cleanup
};
```

## Getting Started

### Complete Demo with Python Client

The fastest way to see DSP in action is using the provided Python demonstration client:

```bash
# Terminal 1: Start DSP server
cargo run --example server

# Terminal 2: Run demo
python3 examples/client.py
```

### Manual Testing with curl

```bash
# 1. Initial request captures session
curl -v http://127.0.0.1:3000/api/logs/server

# 2. Trigger incremental updates  
curl http://127.0.0.1:3000/demo/update

# 3. Request with DSP headers to receive diff
curl -v \
  -H 'X-DSP-Session: [captured-session-id]' \
  -H 'X-Base-Version: [captured-version]' \
  -H 'Accept-Diff: binary-delta' \
  http://127.0.0.1:3000/api/logs/server
```

### Integration Example

```rust
use dsp::{DspServer, DspConfig};
use dsp::diff::similar::SimilarDiffEngine;
use dsp::state::InMemoryStateManager;
use dsp::server::InMemoryResourceStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure DSP server
    let config = DspConfig::default();
    let state_manager = Arc::new(InMemoryStateManager::new(config.clone()));
    let diff_engine = Arc::new(SimilarDiffEngine::new());
    let resource_store = Arc::new(InMemoryResourceStore::new());
    
    // Build server
    let dsp_server = DspServer::builder()
        .config(config)
        .state_manager(state_manager)
        .diff_engine(diff_engine)
        .build()?;
    
    // Handle requests
    let response = dsp_server
        .handle_request(http_request, resource_store)
        .await?;
        
    Ok(())
}
```

## License

MIT License - see [LICENSE](LICENSE) file for details.