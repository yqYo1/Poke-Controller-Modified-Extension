//! Axum lifecycle and generated public API contracts.

pub mod api;
pub mod backend;
pub mod openapi;
pub mod paths;
pub mod realtime;
pub mod realtime_connection;
pub mod rest;
pub mod router;
pub mod security;
pub mod state;
pub mod static_files;
pub mod webrtc;
pub mod websocket;

use std::io;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// A TCP listener that has completed binding but has not begun serving.
#[derive(Debug)]
pub struct BoundServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    router: Router,
}

impl BoundServer {
    /// Binds the HTTP server without exposing any provisional public routes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the requested address cannot be bound.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        Self::bind_with_router(address, Router::new()).await
    }

    /// Binds a fully constructed router without beginning to serve it.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the requested address cannot be bound.
    pub async fn bind_with_router(address: SocketAddr, router: Router) -> io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
            router,
        })
    }

    /// Returns the bound address, including an assigned ephemeral port.
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serves the configured router until cancellation.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if axum cannot serve the bound listener.
    pub async fn serve(self, shutdown: CancellationToken) -> io::Result<()> {
        axum::serve(self.listener, self.router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    use super::BoundServer;

    #[tokio::test]
    async fn server_stops_after_cancellation() {
        let server = BoundServer::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .expect("ephemeral localhost binding must succeed");
        assert_ne!(server.local_addr().port(), 0);

        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));
        shutdown.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .expect("server shutdown must be bounded")
            .expect("server task must not panic")
            .expect("server must stop cleanly");
    }
}
