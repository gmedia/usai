//! hyper 1 server: persistent listener, one connection task per socket,
//! graceful shutdown that lets in-flight worlds settle.

use std::sync::Arc;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

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
    let graceful = hyper_util::server::graceful::GracefulShutdown::new();
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
        let connection = http1::Builder::new().keep_alive(true).serve_connection(
            TokioIo::new(stream),
            service_fn(move |request| {
                let host = Arc::clone(&host);
                async move { Ok::<_, std::convert::Infallible>(host.handle(request).await) }
            }),
        );
        let connection = graceful.watch(connection);
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::debug!(%peer, error = %e, "connection ended with error");
            }
        });
    }
    tracing::info!("http listener closed; draining connections");
    tokio::select! {
        _ = graceful.shutdown() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
            tracing::warn!("connections did not drain within 30s");
        }
    }
    Ok(())
}
