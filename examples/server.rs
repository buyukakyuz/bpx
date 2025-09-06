//! Demo BPX server

use bpx::protocol::headers::BpxHeaders;
use bpx::{
    BpxConfig, BpxServer, ResourcePath, diff::similar::SimilarDiffEngine,
    server::InMemoryResourceStore, state::InMemoryStateManager,
};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request, Response, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::time;

/// Demo data
fn setup_demo_resources(store: &InMemoryResourceStore) {
    let log_stream = (1..=200).map(|i| {
        format!("[2024-01-15T10:{:02}:00.000Z] INFO server.rs:{} Request processed successfully - user_id={}, endpoint=/api/data, duration={}ms, status=200", 
                i % 60, 100 + i, 1000 + (i % 100), 50 + (i % 200))
    }).collect::<Vec<_>>().join("\n");

    store.set_resource(
        ResourcePath::new("/api/logs/server".to_string()),
        Bytes::from(log_stream),
    );

    // Live metrics dashboard
    let metrics_dashboard = format!(r#"{{
  "timestamp": "2024-01-15T10:00:00Z",
  "server_metrics": {{
    "cpu_usage": 45.2,
    "memory_usage": 67.8,
    "disk_usage": 23.1,
    "network_in": 1024,
    "network_out": 2048,
    "active_connections": 42,
    "requests_per_second": 150,
    "error_rate": 0.05
  }},
  "application_metrics": {{
    "total_users": 10000,
    "active_sessions": 250,
    "database_connections": 15,
    "cache_hit_rate": 94.5,
    "queue_size": 5,
    "processed_jobs": 1250
  }},
  "detailed_stats": [
{}
  ]
}}"#, (1..=50).map(|i| format!(
    r#"    {{"endpoint": "/api/endpoint{}", "requests": {}, "avg_response_time": {}ms, "error_count": {}}}"#,
    i, 100 + i * 10, 50 + i * 2, i % 5
)).collect::<Vec<_>>().join(",\n"));

    store.set_resource(
        ResourcePath::new("/api/dashboard/metrics".to_string()),
        Bytes::from(metrics_dashboard),
    );

    // Simple collaborative document for testing
    let collaborative_doc = format!(
        "{{\"document_id\":\"doc_123\",\"title\":\"Team Planning Document\",\"content\":\"Meeting notes with {} attendees discussing {} features and {} action items. This document will be updated incrementally to demonstrate BPX diff capabilities in text editing scenarios.\",\"metadata\":{{\"version\":1,\"word_count\":{},\"last_modified\":\"2024-01-15T10:00:00Z\"}}}}",
        4, 10, 12, 250
    );

    store.set_resource(
        ResourcePath::new("/api/documents/collaborative".to_string()),
        Bytes::from(collaborative_doc),
    );

    println!("Demo resources initialized:");
    println!("  - /api/logs/server (~15KB log stream, perfect for append-only diffs)");
    println!("  - /api/dashboard/metrics (~3KB live metrics, incremental updates)");
    println!("  - /api/documents/collaborative (~5KB document, text editing simulation)");
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    bpx_server: Arc<BpxServer>,
    resource_store: Arc<InMemoryResourceStore>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method();
    let uri = req.uri().clone();

    if method == Method::OPTIONS {
        let response = Response::builder()
            .status(200)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            .header(
                "Access-Control-Allow-Headers",
                "Content-Type, X-BPX-Session, X-Base-Version, Accept-Diff",
            )
            .header("Access-Control-Max-Age", "3600")
            .body(Full::new(Bytes::new()))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())));
        return Ok(response);
    }

    if method != Method::GET {
        let response = Response::builder()
            .status(405)
            .header("Content-Type", "text/plain")
            .header("Access-Control-Allow-Origin", "*")
            .body(Full::new(Bytes::from("Method not allowed")))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("Error"))));
        return Ok(response);
    }

    // Special endpoints for testing
    match uri.path() {
        "/health" => {
            let response = Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(
                    r#"{"status":"healthy","protocol":"BPX/1.0"}"#,
                )))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())));
            return Ok(response);
        }
        "/stats" => {
            let config = bpx_server.config();
            let stats = format!(
                r#"{{"resources":{},"versions":{},"config":{{"max_sessions":{},"session_ttl":{},"max_diff_size":{}}}}}"#,
                resource_store.resource_count(),
                resource_store.version_count(),
                config.max_sessions,
                config.session_ttl.as_secs(),
                config.max_diff_size
            );
            let response = Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(stats)))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())));
            return Ok(response);
        }
        "/demo/update" => {
            // Incremental updates for BPX demonstration
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // 1. Append to log stream
            let current_logs = resource_store
                .get_current_resource(&ResourcePath::new("/api/logs/server".to_string()))
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .unwrap_or_default();

            let new_log_entry = format!(
                "[2024-01-15T10:{:02}:00.000Z] INFO server.rs:{} New request processed - user_id={}, endpoint=/api/update, duration={}ms, status=200",
                (current_time % 60),
                300 + current_time,
                2000 + (current_time % 50),
                45 + (current_time % 100)
            );
            let updated_logs = format!("{}\n{}", current_logs, new_log_entry);

            resource_store.set_resource(
                ResourcePath::new("/api/logs/server".to_string()),
                Bytes::from(updated_logs),
            );

            // 2. Update metrics incrementally
            let metrics_update = format!(r#"{{
  "timestamp": "2024-01-15T10:{:02}:00Z",
  "server_metrics": {{
    "cpu_usage": {:.1},
    "memory_usage": {:.1},
    "disk_usage": 23.1,
    "network_in": {},
    "network_out": {},
    "active_connections": {},
    "requests_per_second": {},
    "error_rate": {:.3}
  }},
  "application_metrics": {{
    "total_users": {},
    "active_sessions": {},
    "database_connections": 15,
    "cache_hit_rate": 94.5,
    "queue_size": {},
    "processed_jobs": {}
  }},
  "detailed_stats": [
{}
  ]
}}"#, 
            current_time % 60,
            45.2 + (current_time % 20) as f32,
            67.8 + (current_time % 15) as f32,
            1024 + current_time * 10,
            2048 + current_time * 15,
            42 + (current_time % 20),
            150 + (current_time % 50),
            0.05 + (current_time % 10) as f32 / 1000.0,
            10000 + current_time,
            250 + (current_time % 100),
            5 + (current_time % 10),
            1250 + current_time * 5,
            (1..=50).map(|i| format!(
                r#"    {{"endpoint": "/api/endpoint{}", "requests": {}, "avg_response_time": {}ms, "error_count": {}}}"#,
                i, 100 + i * 10 + (current_time % 50), 50 + i * 2, (i as u64 + current_time) % 5
            )).collect::<Vec<_>>().join(",\n")
            );

            resource_store.set_resource(
                ResourcePath::new("/api/dashboard/metrics".to_string()),
                Bytes::from(metrics_update),
            );

            // 3. Simulate collaborative document editing (text insertions)
            let current_doc = resource_store
                .get_current_resource(&ResourcePath::new(
                    "/api/documents/collaborative".to_string(),
                ))
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .unwrap_or_default();

            let mut updated_doc = current_doc.clone();
            // Simulate adding a comment or edit
            if updated_doc.contains("\"version\":1") {
                updated_doc = updated_doc.replace("\"version\":1", "\"version\":2");
                updated_doc = updated_doc.replace("demonstrate BPX diff capabilities", 
                    &format!("demonstrate BPX diff capabilities. EDIT #{}: Added incremental change at timestamp {}", current_time % 100, current_time));
            } else {
                updated_doc = updated_doc.replace(
                    "text editing scenarios",
                    &format!(
                        "text editing scenarios with update #{}",
                        current_time % 1000
                    ),
                );
            }

            resource_store.set_resource(
                ResourcePath::new("/api/documents/collaborative".to_string()),
                Bytes::from(updated_doc),
            );

            let response = Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(r#"{"message":"Incremental updates applied","updated":["logs","metrics","document"],"optimized_for":"BPX differential sync"}"#)))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())));
            return Ok(response);
        }
        _ => {}
    }

    // Handle BPX requests through the integrated server
    match bpx_server
        .handle_request(req, Arc::clone(&resource_store))
        .await
    {
        Ok(response) => {
            // Convert Bytes to Full<Bytes> and add CORS headers
            let (parts, body) = response.into_parts();
            let mut response = Response::from_parts(parts, Full::new(body));

            // Add CORS headers
            response.headers_mut().insert(
                hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                "*".parse().unwrap(),
            );
            let expose = BpxHeaders::all().join(",");
            response.headers_mut().insert(
                hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
                expose.parse().unwrap(),
            );

            Ok(response)
        }
        Err(err) => {
            eprintln!("BPX error for {}: {}", uri.path(), err);
            let response = Response::builder()
                .status(500)
                .header("Content-Type", "text/plain")
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(format!("BPX Error: {}", err))))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::from("Internal Server Error"))));
            Ok(response)
        }
    }
}

/// Cleanup task that runs periodically
async fn cleanup_task(bpx_server: Arc<BpxServer>) {
    let interval_secs = bpx_server.config().cleanup_interval.as_secs().max(1);
    let mut interval = time::interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;
        bpx_server.cleanup_expired_sessions().await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Starting BPX Server...");

    // Create configuration
    let config = BpxConfig {
        max_sessions: 10_000,
        max_resources_per_session: 100,
        session_ttl: Duration::from_secs(30 * 60), // 30 minutes
        max_diff_size: 5 * 1024 * 1024,            // 5MB
        min_compression_ratio: 0.1,                // 10% savings required
        cleanup_interval: Duration::from_secs(60),
    };

    let state_manager = Arc::new(InMemoryStateManager::new(config.clone()));
    let diff_engine = Arc::new(SimilarDiffEngine::with_compression_ratio(
        config.min_compression_ratio,
    ));
    let resource_store = Arc::new(InMemoryResourceStore::new());

    setup_demo_resources(&resource_store);

    let bpx_server = Arc::new(
        BpxServer::builder()
            .config(config)
            .state_manager(state_manager)
            .diff_engine(diff_engine)
            .build()?,
    );

    println!("BPX Server components initialized");

    let cleanup_server = Arc::clone(&bpx_server);
    tokio::spawn(async move {
        cleanup_task(cleanup_server).await;
    });

    let service = {
        let bpx_server = Arc::clone(&bpx_server);
        let resource_store = Arc::clone(&resource_store);

        service_fn(move |req| {
            handle_request(req, Arc::clone(&bpx_server), Arc::clone(&resource_store))
        })
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("BPX Server listening on http://127.0.0.1:3000");
    println!();
    println!("Available endpoints:");
    println!("  /health                   - Server health check");
    println!("  /stats                    - Server statistics");
    println!("  /demo/update              - Apply incremental updates");
    println!("  /api/logs/server          - Append-only log stream (great for BPX)");
    println!("  /api/dashboard/metrics    - Live metrics (line-based demo)");
    println!("  /api/documents/collaborative - Collaborative doc (single-line JSON)");
    println!();
    println!("BPX Protocol Test Commands:");
    println!();
    println!("1. Get initial resource and capture session/version:");
    println!("   curl -v http://127.0.0.1:3000/api/logs/server");
    println!();
    println!("2. Request with BPX headers:");
    println!(
        "   curl -v -H 'X-BPX-Session: <SESSION>' -H 'X-Base-Version: <VERSION>' -H 'Accept-Diff: binary-delta' http://127.0.0.1:3000/api/logs/server"
    );
    println!();
    println!("3. Update the resource:");
    println!("   curl http://127.0.0.1:3000/demo/update");
    println!();
    println!("4. Request again with same session/version to see diff:");
    println!(
        "   curl -v -H 'X-BPX-Session: <SESSION>' -H 'X-Base-Version: <VERSION>' -H 'Accept-Diff: binary-delta' http://127.0.0.1:3000/api/logs/server"
    );
    println!();

    loop {
        let (stream, _addr) = listener.accept().await?;

        let io = TokioIo::new(stream);
        let service = service.clone();

        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
