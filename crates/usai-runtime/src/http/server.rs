//! hyper 1 server: persistent listener, one connection task per socket,
//! graceful shutdown that lets in-flight worlds settle. Connections are
//! upgradeable (WebSocket).

use std::sync::Arc;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::pipeline::HttpHost;

/// Binds and serves until `shutdown` fires. Returns the bound address via
/// `on_bound` before the first accept, so tests can use port 0.
pub async fn serve(
    host: Arc<HttpHost>,
    shutdown: CancellationToken,
    on_bound: impl FnOnce(std::net::SocketAddr),
) -> std::io::Result<()> {
    let listener = TcpListener::bind(host.config().addr).await?;
    on_bound(listener.local_addr()?);
    let tracker = TaskTracker::new();
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    continue;
                }
            },
            _ = shutdown.cancelled() => break,
        };
        let host = Arc::clone(&host);
        let shutdown = shutdown.clone();
        tracker.spawn(async move {
            let connection = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(
                    TokioIo::new(stream),
                    service_fn(move |request| {
                        let host = Arc::clone(&host);
                        async move { Ok::<_, std::convert::Infallible>(host.handle(request).await) }
                    }),
                )
                // A 101 hands the socket to the WebSocket pump.
                .with_upgrades();
            tokio::pin!(connection);
            tokio::select! {
                result = connection.as_mut() => {
                    if let Err(e) = result {
                        tracing::debug!(%peer, error = %e, "connection ended with error");
                    }
                }
                _ = shutdown.cancelled() => {
                    // Finish the in-flight response, then close.
                    connection.as_mut().graceful_shutdown();
                    if let Err(e) = connection.await {
                        tracing::debug!(%peer, error = %e, "connection ended during shutdown");
                    }
                }
            }
        });
    }
    tracing::info!("http listener closed; draining connections");
    tracker.close();
    tokio::select! {
        _ = tracker.wait() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
            tracing::warn!("connections did not drain within 30s");
        }
    }
    Ok(())
}

/// A listener that serves only the runtime-owned `/_usai/` surfaces
/// (status, metrics, live, ready, docs) — for a private interface, so the
/// application listener never exposes them (`usai run --status-addr`).
pub async fn serve_internal(
    host: Arc<HttpHost>,
    addr: std::net::SocketAddr,
    shutdown: CancellationToken,
    on_bound: impl FnOnce(std::net::SocketAddr),
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    on_bound(listener.local_addr()?);
    let tracker = TaskTracker::new();
    loop {
        let (stream, _peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(_) => continue,
            },
            _ = shutdown.cancelled() => break,
        };
        let host = Arc::clone(&host);
        tracker.spawn(async move {
            let _ = http1::Builder::new()
                .serve_connection(
                    TokioIo::new(stream),
                    service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                        let host = Arc::clone(&host);
                        async move {
                            let (parts, _) = request.into_parts();
                            let response = if parts.method == hyper::Method::GET {
                                host.internal(parts.uri.path(), &parts.headers, true, true).await
                            } else {
                                None
                            };
                            Ok::<_, std::convert::Infallible>(response.unwrap_or_else(|| {
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::NOT_FOUND)
                                    .header(hyper::header::CONTENT_TYPE, "application/json")
                                    .body(
                                        http_body_util::BodyExt::boxed(http_body_util::Full::new(bytes::Bytes::from_static(
                                            br#"{"error":{"code":"route_not_found","message":"this listener serves /_usai/status, /_usai/metrics, /_usai/live, /_usai/ready, /_usai/docs and /_usai/openapi.json"}}"#,
                                        ))),
                                    )
                                    .expect("static response")
                            }))
                        }
                    }),
                )
                .await;
        });
    }
    tracker.close();
    tracker.wait().await;
    Ok(())
}
