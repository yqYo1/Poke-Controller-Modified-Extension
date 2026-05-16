use std::io;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tracing::{debug, info};

/// Socket communication error types
#[derive(Error, Debug)]
pub enum SocketError {
    #[error("Failed to connect to {addr}: {source}")]
    ConnectError { addr: String, source: io::Error },

    #[error("Failed to send data: {0}")]
    SendError(io::Error),

    #[error("Failed to receive data: {0}")]
    ReceiveError(io::Error),

    #[error("Connection closed by peer")]
    ConnectionClosed,

    #[error("Socket is not connected")]
    NotConnected,

    #[error("Timeout: {0}")]
    Timeout(String),
}

/// A TCP socket client using `tokio::net::TcpStream`
///
/// Provides async connect, send, receive, and disconnect operations
/// with configurable timeouts.
pub struct SocketClient {
    /// Buffered writer for sending data
    writer: Option<BufWriter<OwnedWriteHalf>>,
    /// Buffered reader for receiving data
    reader: Option<BufReader<OwnedReadHalf>>,
    /// Connection address for reconnection info
    addr: Option<String>,
    /// Receive timeout duration
    recv_timeout: Option<Duration>,
}

impl SocketClient {
    /// Create a new unconnected SocketClient
    pub fn new() -> Self {
        Self {
            writer: None,
            reader: None,
            addr: None,
            recv_timeout: None,
        }
    }

    /// Create a new SocketClient with a receive timeout
    pub fn with_timeout(recv_timeout: Duration) -> Self {
        Self {
            writer: None,
            reader: None,
            addr: None,
            recv_timeout: Some(recv_timeout),
        }
    }

    /// Connect to a TCP server at the given address (host:port)
    pub async fn connect(&mut self, addr: &str) -> Result<(), SocketError> {
        info!("Connecting to TCP socket: {}", addr);

        let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addr))
            .await
            .map_err(|_| SocketError::Timeout(format!("Connection timeout to {}", addr)))?
            .map_err(|e| SocketError::ConnectError {
                addr: addr.to_string(),
                source: e,
            })?;

        let (r, w) = stream.into_split();
        self.writer = Some(BufWriter::new(w));
        self.reader = Some(BufReader::new(r));
        self.addr = Some(addr.to_string());

        info!("Connected to TCP socket: {}", addr);
        Ok(())
    }

    /// Send raw bytes over the socket connection
    pub async fn send(&mut self, data: &[u8]) -> Result<(), SocketError> {
        let writer = self.writer.as_mut().ok_or(SocketError::NotConnected)?;

        debug!("Sending {} bytes", data.len());
        writer
            .write_all(data)
            .await
            .map_err(SocketError::SendError)?;
        writer.flush().await.map_err(SocketError::SendError)?;

        Ok(())
    }

    /// Send a string message over the socket (appends newline)
    pub async fn send_line(&mut self, line: &str) -> Result<(), SocketError> {
        let data = format!("{}\n", line);
        self.send(data.as_bytes()).await
    }

    /// Receive raw bytes up to the specified buffer size
    pub async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, SocketError> {
        let reader = self.reader.as_mut().ok_or(SocketError::NotConnected)?;

        let read_result = if let Some(timeout) = self.recv_timeout {
            tokio::time::timeout(timeout, reader.read(buf))
                .await
                .map_err(|_| SocketError::Timeout("Receive timeout".to_string()))?
                .map_err(SocketError::ReceiveError)?
        } else {
            reader.read(buf).await.map_err(SocketError::ReceiveError)?
        };

        match read_result {
            0 => Err(SocketError::ConnectionClosed),
            n => Ok(n),
        }
    }

    /// Receive a line of text (up to newline) from the socket
    pub async fn receive_line(&mut self) -> Result<String, SocketError> {
        let reader = self.reader.as_mut().ok_or(SocketError::NotConnected)?;

        let result = if let Some(timeout) = self.recv_timeout {
            let mut line = String::new();
            tokio::time::timeout(timeout, reader.read_line(&mut line))
                .await
                .map_err(|_| SocketError::Timeout("Receive timeout".to_string()))?
                .map_err(SocketError::ReceiveError)?;
            Ok(line)
        } else {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .map_err(SocketError::ReceiveError)?;
            Ok(line)
        }?;

        if result.is_empty() {
            return Err(SocketError::ConnectionClosed);
        }

        Ok(result
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string())
    }

    /// Close the socket connection
    pub fn close(&mut self) {
        debug!("Closing socket connection to {:?}", self.addr);
        self.writer = None;
        self.reader = None;
        self.addr = None;
    }

    /// Check if the socket is currently connected
    pub fn is_connected(&self) -> bool {
        self.writer.is_some()
    }

    /// Get the remote address of the connection
    pub fn addr(&self) -> Option<&str> {
        self.addr.as_deref()
    }
}

impl Default for SocketClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Helper: start an echo server that repeats back what it receives
    async fn start_echo_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (r, w) = stream.into_split();
                let mut reader = BufReader::new(r);
                let mut writer = BufWriter::new(w);
                let mut buf = vec![0u8; 1024];

                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            writer.write_all(&buf[..n]).await.unwrap();
                            writer.flush().await.unwrap();
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        addr
    }

    #[tokio::test]
    async fn test_connect_and_disconnect() {
        let addr = start_echo_server().await;
        let mut client = SocketClient::new();

        assert!(!client.is_connected());
        client.connect(&addr).await.unwrap();
        assert!(client.is_connected());
        assert_eq!(client.addr(), Some(addr.as_str()));

        client.close();
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_send_and_receive() {
        let addr = start_echo_server().await;
        let mut client = SocketClient::new();
        client.connect(&addr).await.unwrap();

        let msg = b"hello world";
        client.send(msg).await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = client.receive(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], msg);

        client.close();
    }

    #[tokio::test]
    async fn test_send_line_and_receive_line() {
        let addr = start_echo_server().await;
        let mut client = SocketClient::new();
        client.connect(&addr).await.unwrap();

        client.send_line("test line").await.unwrap();
        let response = client.receive_line().await.unwrap();
        assert_eq!(response, "test line");

        client.close();
    }

    #[tokio::test]
    async fn test_connect_error() {
        let mut client = SocketClient::new();
        let result = client.connect("127.0.0.1:1").await;
        assert!(result.is_err());
        match result {
            Err(SocketError::ConnectError { .. }) => {}
            _ => panic!("Expected ConnectError"),
        }
    }

    #[tokio::test]
    async fn test_send_without_connect() {
        let mut client = SocketClient::new();
        let result = client.send(b"data").await;
        assert!(matches!(result, Err(SocketError::NotConnected)));
    }

    #[tokio::test]
    async fn test_default() {
        let client = SocketClient::default();
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_with_timeout() {
        let timeout = Duration::from_millis(100);
        let client = SocketClient::with_timeout(timeout);
        assert!(!client.is_connected());
    }
}
