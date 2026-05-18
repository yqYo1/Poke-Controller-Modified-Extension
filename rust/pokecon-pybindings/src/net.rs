use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use pokecon_core::net::mqtt::MqttClient as RustMqttClient;
use pokecon_core::net::mqtt::MqttConfig as RustMqttConfig;
use pokecon_core::net::mqtt::MqttMessage as RustMqttMessage;
use pokecon_core::net::socket::SocketClient as RustSocketClient;

// ============================================================
// Global tokio runtime  (shared pattern with sender/keys/notify)
// ============================================================

fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

// ============================================================
// QoS Level enum for MQTT
// ============================================================

/// MQTT Quality of Service level.
///
/// Python usage::
///
///     from pokecon.net import QoS
///     qos = QoS.AT_MOST_ONCE   # QoS 0
///     qos = QoS.AT_LEAST_ONCE  # QoS 1
///     qos = QoS.EXACTLY_ONCE   # QoS 2
#[pyclass(eq, eq_int)]
#[derive(Clone, PartialEq)]
pub enum PyQoS {
    AT_MOST_ONCE = 0,
    AT_LEAST_ONCE = 1,
    EXACTLY_ONCE = 2,
}

impl From<PyQoS> for rumqttc::QoS {
    fn from(qos: PyQoS) -> Self {
        match qos {
            PyQoS::AT_MOST_ONCE => rumqttc::QoS::AtMostOnce,
            PyQoS::AT_LEAST_ONCE => rumqttc::QoS::AtLeastOnce,
            PyQoS::EXACTLY_ONCE => rumqttc::QoS::ExactlyOnce,
        }
    }
}

// ============================================================
// HttpResponse
// ============================================================

/// Represents an HTTP response from the HttpClient.
///
/// Python usage::
///
///     from pokecon.net import HttpClient
///     client = HttpClient()
///     resp = client.get("https://httpbin.org/get")
///     assert resp.status == 200
///     data = resp.json()
#[pyclass(name = "HttpResponse")]
#[derive(Clone)]
pub struct PyHttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[pymethods]
impl PyHttpResponse {
    /// HTTP status code (e.g. 200, 404, 500).
    #[getter]
    fn status(&self) -> u16 {
        self.status
    }

    /// Response headers as a dict.
    #[getter]
    fn headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    /// Response body as raw bytes.
    #[getter]
    fn body(&self) -> Vec<u8> {
        self.body.clone()
    }

    /// Decode response body as UTF-8 text.
    fn text(&self) -> PyResult<String> {
        String::from_utf8(self.body.clone())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to decode body as UTF-8: {e}")))
    }

    /// Decode response body as JSON.
    fn json(&self, py: Python<'_>) -> PyResult<PyObject> {
        let value: serde_json::Value = serde_json::from_slice(&self.body)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to decode body as JSON: {e}")))?;
        // Convert serde_json::Value to Python object via serde_json -> String -> Python json module
        let json_str = serde_json::to_string(&value)
            .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization failed: {e}")))?;
        let json_module = py.import("json")?;
        let result = json_module.call_method1("loads", (json_str,))?;
        Ok(result.into())
    }

    fn __repr__(&self) -> String {
        format!("<HttpResponse status={}>", self.status)
    }
}

// ============================================================
// HttpClient
// ============================================================

/// HTTP client supporting GET, POST, PUT, DELETE operations.
///
/// Uses ``reqwest`` under the hood with connection pooling and TLS support.
///
/// Python usage::
///
///     from pokecon.net import HttpClient
///
///     client = HttpClient()
///
///     # GET request
///     resp = client.get("https://httpbin.org/get")
///     print(resp.status, resp.text())
///
///     # POST with JSON body
///     resp = client.post(
///         "https://httpbin.org/post",
///         json={"key": "value"},
///     )
///
///     # POST with form body
///     resp = client.post(
///         "https://httpbin.org/post",
///         body=b"raw data",
///     )
///
///     # PUT request
///     resp = client.put(
///         "https://httpbin.org/put",
///         json={"updated": True},
///     )
///
///     # DELETE request
///     resp = client.delete("https://httpbin.org/delete")
///
///     # Custom headers
///     resp = client.get(
///         "https://httpbin.org/headers",
///         headers={"Authorization": "Bearer token123"},
///     )
///
///     # With timeout
///     resp = client.get("https://httpbin.org/delay/10", timeout=5.0)
#[pyclass(name = "HttpClient")]
pub struct PyHttpClient {
    client: reqwest::blocking::Client,
}

#[pymethods]
impl PyHttpClient {
    /// Create a new HttpClient.
    ///
    /// Parameters
    /// ----------
    /// timeout : float, optional
    ///     Default request timeout in seconds (default: 30.0).
    #[new]
    #[pyo3(signature = (timeout = None))]
    fn new(timeout: Option<f64>) -> Self {
        let mut builder = reqwest::blocking::Client::builder().tls_built_in_root_certs(true);
        if let Some(t) = timeout {
            builder = builder.timeout(Duration::from_secs_f64(t));
        }
        Self {
            client: builder.build().expect("Failed to create HTTP client"),
        }
    }

    /// Perform an HTTP GET request.
    ///
    /// Parameters
    /// ----------
    /// url : str
    ///     Target URL.
    /// headers : dict, optional
    ///     Custom HTTP headers.
    /// timeout : float, optional
    ///     Per-request timeout in seconds (overrides client default).
    #[pyo3(signature = (url, headers = None, timeout = None))]
    fn get(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
        timeout: Option<f64>,
    ) -> PyResult<PyHttpResponse> {
        let mut req = self.client.get(url);
        if let Some(h) = headers {
            for (k, v) in h {
                req = req.header(&k, &v);
            }
        }
        if let Some(t) = timeout {
            req = req.timeout(Duration::from_secs_f64(t));
        }
        execute_request(req)
    }

    /// Perform an HTTP POST request.
    ///
    /// Parameters
    /// ----------
    /// url : str
    ///     Target URL.
    /// body : bytes, optional
    ///     Raw request body bytes.
    /// json : any, optional
    ///     JSON-serializable request body (takes precedence over ``body``).
    /// headers : dict, optional
    ///     Custom HTTP headers.
    /// timeout : float, optional
    ///     Per-request timeout in seconds (overrides client default).
    #[pyo3(signature = (url, body = None, json = None, headers = None, timeout = None))]
    fn post(
        &self,
        url: &str,
        body: Option<Vec<u8>>,
        json: Option<PyObject>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<f64>,
    ) -> PyResult<PyHttpResponse> {
        let mut req = self.client.post(url);
        if let Some(j) = json {
            // Convert Python object to JSON string and then use it
            let json_str = Python::with_gil(|py| -> PyResult<String> {
                let json_module = py.import("json")?;
                let s = json_module.call_method1("dumps", (j,))?;
                s.extract::<String>()
            })?;
            req = req.header("Content-Type", "application/json");
            req = req.body(json_str);
        } else if let Some(b) = body {
            req = req.body(b);
        }
        if let Some(h) = headers {
            for (k, v) in h {
                req = req.header(&k, &v);
            }
        }
        if let Some(t) = timeout {
            req = req.timeout(Duration::from_secs_f64(t));
        }
        execute_request(req)
    }

    /// Perform an HTTP PUT request.
    ///
    /// Parameters
    /// ----------
    /// url : str
    ///     Target URL.
    /// body : bytes, optional
    ///     Raw request body bytes.
    /// json : any, optional
    ///     JSON-serializable request body (takes precedence over ``body``).
    /// headers : dict, optional
    ///     Custom HTTP headers.
    /// timeout : float, optional
    ///     Per-request timeout in seconds (overrides client default).
    #[pyo3(signature = (url, body = None, json = None, headers = None, timeout = None))]
    fn put(
        &self,
        url: &str,
        body: Option<Vec<u8>>,
        json: Option<PyObject>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<f64>,
    ) -> PyResult<PyHttpResponse> {
        let mut req = self.client.put(url);
        if let Some(j) = json {
            let json_str = Python::with_gil(|py| -> PyResult<String> {
                let json_module = py.import("json")?;
                let s = json_module.call_method1("dumps", (j,))?;
                s.extract::<String>()
            })?;
            req = req.header("Content-Type", "application/json");
            req = req.body(json_str);
        } else if let Some(b) = body {
            req = req.body(b);
        }
        if let Some(h) = headers {
            for (k, v) in h {
                req = req.header(&k, &v);
            }
        }
        if let Some(t) = timeout {
            req = req.timeout(Duration::from_secs_f64(t));
        }
        execute_request(req)
    }

    /// Perform an HTTP DELETE request.
    ///
    /// Parameters
    /// ----------
    /// url : str
    ///     Target URL.
    /// headers : dict, optional
    ///     Custom HTTP headers.
    /// timeout : float, optional
    ///     Per-request timeout in seconds (overrides client default).
    #[pyo3(signature = (url, headers = None, timeout = None))]
    fn delete(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
        timeout: Option<f64>,
    ) -> PyResult<PyHttpResponse> {
        let mut req = self.client.delete(url);
        if let Some(h) = headers {
            for (k, v) in h {
                req = req.header(&k, &v);
            }
        }
        if let Some(t) = timeout {
            req = req.timeout(Duration::from_secs_f64(t));
        }
        execute_request(req)
    }

    fn __repr__(&self) -> String {
        "<HttpClient>".to_string()
    }
}

fn execute_request(req: reqwest::blocking::RequestBuilder) -> PyResult<PyHttpResponse> {
    let response = req
        .send()
        .map_err(|e| PyRuntimeError::new_err(format!("HTTP request failed: {e}")))?;

    let status = response.status().as_u16();
    let headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let body = response
        .bytes()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to read response body: {e}")))?;

    Ok(PyHttpResponse {
        status,
        headers,
        body: body.to_vec(),
    })
}

// ============================================================
// SocketClient  —  wraps pokecon_core::net::SocketClient
// ============================================================

/// A TCP socket client for raw socket communication.
///
/// Provides connect, send, receive, and close operations on TCP sockets.
///
/// Python usage::
///
///     from pokecon.net import SocketClient
///
///     sock = SocketClient()
///     sock.connect("127.0.0.1:8080")
///     sock.send(b"Hello, server!")
///     data = sock.receive(1024)
///     print(data)
///     sock.close()
#[pyclass(name = "SocketClient")]
pub struct PySocketClient {
    inner: RustSocketClient,
}

#[pymethods]
impl PySocketClient {
    /// Create a new unconnected SocketClient.
    ///
    /// Parameters
    /// ----------
    /// recv_timeout : float, optional
    ///     Receive timeout in seconds (default: no timeout).
    #[new]
    #[pyo3(signature = (recv_timeout = None))]
    fn new(recv_timeout: Option<f64>) -> Self {
        let inner = if let Some(t) = recv_timeout {
            RustSocketClient::with_timeout(Duration::from_secs_f64(t))
        } else {
            RustSocketClient::new()
        };
        Self { inner }
    }

    /// Connect to a TCP server.
    ///
    /// Parameters
    /// ----------
    /// addr : str
    ///     Address in ``host:port`` format (e.g. ``"127.0.0.1:8080"``).
    fn connect(&mut self, addr: &str) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.connect(addr))
            .map_err(|e| PyRuntimeError::new_err(format!("Socket connect failed: {e}")))
    }

    /// Send raw bytes over the socket.
    ///
    /// Parameters
    /// ----------
    /// data : bytes
    ///     Bytes to send.
    fn send(&mut self, data: Vec<u8>) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.send(&data))
            .map_err(|e| PyRuntimeError::new_err(format!("Socket send failed: {e}")))
    }

    /// Send a string line (appends newline).
    ///
    /// Parameters
    /// ----------
    /// line : str
    ///     Text line to send.
    fn send_line(&mut self, line: &str) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.send_line(line))
            .map_err(|e| PyRuntimeError::new_err(format!("Socket send_line failed: {e}")))
    }

    /// Receive raw bytes up to the specified buffer size.
    ///
    /// Parameters
    /// ----------
    /// bufsize : int
    ///     Maximum number of bytes to receive.
    ///
    /// Returns
    /// -------
    /// bytes
    ///     Received bytes.
    fn receive(&mut self, bufsize: usize) -> PyResult<Vec<u8>> {
        let rt = global_runtime();
        let mut buf = vec![0u8; bufsize];
        let n = rt
            .block_on(self.inner.receive(&mut buf))
            .map_err(|e| PyRuntimeError::new_err(format!("Socket receive failed: {e}")))?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Receive a line of text from the socket.
    ///
    /// Returns
    /// -------
    /// str
    ///     Received line (without trailing newline).
    fn receive_line(&mut self) -> PyResult<String> {
        let rt = global_runtime();
        rt.block_on(self.inner.receive_line())
            .map_err(|e| PyRuntimeError::new_err(format!("Socket receive_line failed: {e}")))
    }

    /// Close the socket connection.
    fn close(&mut self) {
        let rt = global_runtime();
        rt.block_on(self.inner.close());
    }

    /// Check whether the socket is currently connected.
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Get the remote address of the connection.
    #[getter]
    fn addr(&self) -> Option<String> {
        self.inner.addr().map(|s| s.to_string())
    }

    fn __repr__(&self) -> String {
        if self.inner.is_connected() {
            format!("<SocketClient connected to {:?}>", self.inner.addr())
        } else {
            "<SocketClient disconnected>".to_string()
        }
    }
}

// ============================================================
// Internal types for WebSocket
// ============================================================

/// Internal message type for WebSocket received messages
#[derive(Debug, Clone)]
struct WebSocketMessage {
    is_text: bool,
    data: Vec<u8>,
}

/// Commands sent to the WebSocket writer task
enum WebSocketCommand {
    Send { is_text: bool, data: Vec<u8> },
    Close,
}

// ============================================================
// WebSocketClient
// ============================================================

/// A WebSocket client for real-time bidirectional communication.
///
/// Supports text and binary messages over WebSocket connections.
///
/// Python usage::
///
///     from pokecon.net import WebSocketClient
///
///     ws = WebSocketClient()
///     ws.connect("wss://echo.websocket.org")
///     ws.send("Hello, WebSocket!")
///     msg = ws.recv()
///     print(msg)
///     ws.close()
#[pyclass(name = "WebSocketClient")]
pub struct PyWebSocketClient {
    /// Sender half for sending messages to the WebSocket writer task
    write_tx: Option<tokio::sync::mpsc::UnboundedSender<WebSocketCommand>>,
    /// Receiver for incoming messages from the reader task
    message_rx: Option<tokio::sync::mpsc::UnboundedReceiver<WebSocketMessage>>,
    /// Handle to the background reader task
    reader_handle: Option<tokio::task::JoinHandle<()>>,
    /// Connection URL for repr
    url: Option<String>,
    /// Whether connected
    connected: bool,
}

#[pymethods]
impl PyWebSocketClient {
    /// Create a new unconnected WebSocket client.
    #[new]
    fn new() -> Self {
        Self {
            write_tx: None,
            message_rx: None,
            reader_handle: None,
            url: None,
            connected: false,
        }
    }

    /// Connect to a WebSocket server.
    ///
    /// Parameters
    /// ----------
    /// url : str
    ///     WebSocket URL (``ws://...`` or ``wss://...``).
    fn connect(&mut self, url: &str) -> PyResult<()> {
        use futures_util::stream::StreamExt;

        let url_owned = url.to_string();
        let rt = global_runtime();

        // Perform the async connect
        let (ws_stream, _) = rt
            .block_on(tokio_tungstenite::connect_async(url))
            .map_err(|e| PyRuntimeError::new_err(format!("WebSocket connect failed: {e}")))?;

        let (write, read) = ws_stream.split();

        // Channel for sending commands to writer
        let (write_tx, mut write_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketCommand>();
        // Channel for receiving incoming messages
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<WebSocketMessage>();

        // Spawn writer task (sends data down the WebSocket)
        tokio::spawn(async move {
            use futures_util::sink::SinkExt;
            let mut write = write;
            while let Some(cmd) = write_rx.recv().await {
                match cmd {
                    WebSocketCommand::Send { is_text, data } => {
                        let msg = if is_text {
                            match String::from_utf8(data) {
                                Ok(text) => tokio_tungstenite::tungstenite::Message::Text(text),
                                Err(e) => {
                                    tokio_tungstenite::tungstenite::Message::Binary(e.into_bytes())
                                }
                            }
                        } else {
                            tokio_tungstenite::tungstenite::Message::Binary(data)
                        };
                        if write.send(msg).await.is_err() {
                            break;
                        }
                    }
                    WebSocketCommand::Close => {
                        let _ = write.close().await;
                        break;
                    }
                }
            }
        });

        // Spawn reader task (reads from WebSocket and forwards to channel)
        let reader_handle = tokio::spawn(async move {
            use futures_util::stream::StreamExt;
            let mut read = read;
            while let Some(Ok(msg)) = read.next().await {
                match msg {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        let ws_msg = WebSocketMessage {
                            is_text: true,
                            data: text.into_bytes(),
                        };
                        if msg_tx.send(ws_msg).is_err() {
                            break;
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Binary(data) => {
                        let ws_msg = WebSocketMessage {
                            is_text: false,
                            data,
                        };
                        if msg_tx.send(ws_msg).is_err() {
                            break;
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Ping(_)
                    | tokio_tungstenite::tungstenite::Message::Pong(_) => {
                        // Pings/pongs handled automatically by tungstenite
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        break;
                    }
                    tokio_tungstenite::tungstenite::Message::Frame(_) => {
                        // Raw frame, ignore
                    }
                }
            }
        });

        self.write_tx = Some(write_tx);
        self.message_rx = Some(msg_rx);
        self.reader_handle = Some(reader_handle);
        self.url = Some(url_owned);
        self.connected = true;

        Ok(())
    }

    /// Send a text message over the WebSocket.
    ///
    /// Parameters
    /// ----------
    /// data : str
    ///     Text message to send.
    fn send(&self, data: &str) -> PyResult<()> {
        let tx = self
            .write_tx
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("WebSocket is not connected"))?;

        tx.send(WebSocketCommand::Send {
            is_text: true,
            data: data.as_bytes().to_vec(),
        })
        .map_err(|_| PyRuntimeError::new_err("WebSocket send channel closed"))?;

        Ok(())
    }

    /// Send a binary message over the WebSocket.
    ///
    /// Parameters
    /// ----------
    /// data : bytes
    ///     Binary data to send.
    fn send_binary(&self, data: Vec<u8>) -> PyResult<()> {
        let tx = self
            .write_tx
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("WebSocket is not connected"))?;

        tx.send(WebSocketCommand::Send {
            is_text: false,
            data,
        })
        .map_err(|_| PyRuntimeError::new_err("WebSocket send channel closed"))?;

        Ok(())
    }

    /// Receive the next message (blocking).
    ///
    /// Parameters
    /// ----------
    /// timeout : float, optional
    ///     Maximum wait time in seconds. If ``None``, waits indefinitely.
    ///
    /// Returns
    /// -------
    /// str, bytes, or None
    ///     Received message content. Returns ``str`` for text messages,
    ///     ``bytes`` for binary messages.
    ///     Returns ``None`` if timeout occurs or connection is closed.
    #[pyo3(signature = (timeout = None))]
    fn recv(&mut self, timeout: Option<f64>) -> PyResult<Option<PyObject>> {
        let rx = self
            .message_rx
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("WebSocket is not connected"))?;

        let msg = if let Some(t) = timeout {
            let rt = global_runtime();
            rt.block_on(async {
                tokio::time::timeout(Duration::from_secs_f64(t), rx.recv())
                    .await
                    .ok()
            })
            .flatten()
        } else {
            let rt = global_runtime();
            rt.block_on(rx.recv())
        };

        match msg {
            Some(ws_msg) => {
                let result: PyObject = Python::with_gil(|py| {
                    if ws_msg.is_text {
                        let text = String::from_utf8_lossy(&ws_msg.data).to_string();
                        pyo3::types::PyString::new(py, &text).into()
                    } else {
                        PyBytes::new(py, &ws_msg.data).into()
                    }
                });
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }

    /// Close the WebSocket connection.
    fn close(&mut self) {
        // Send close command to writer task
        if let Some(tx) = self.write_tx.take() {
            let _ = tx.send(WebSocketCommand::Close);
        }

        // Abort the reader task
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }

        self.connected = false;
        self.message_rx = None;
        self.url = None;
    }

    /// Check whether the WebSocket is currently connected.
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn __repr__(&self) -> String {
        if self.connected {
            format!("<WebSocketClient connected to {:?}>", self.url)
        } else {
            "<WebSocketClient disconnected>".to_string()
        }
    }
}

impl Drop for PyWebSocketClient {
    fn drop(&mut self) {
        self.close();
    }
}

// ============================================================
// MqttMessage  —  wraps pokecon_core::net::MqttMessage
// ============================================================

/// Represents an incoming MQTT message.
///
/// Python usage::
///
///     from pokecon.net import MqttClient
///     client = MqttClient("localhost", 1883)
///     client.connect()
///     client.subscribe("test/topic")
///     msg = client.recv()
///     print(msg.topic, msg.payload)
#[pyclass(name = "MqttMessage")]
#[derive(Clone)]
pub struct PyMqttMessage {
    inner: RustMqttMessage,
}

#[pymethods]
impl PyMqttMessage {
    /// The topic the message was received on.
    #[getter]
    fn topic(&self) -> String {
        self.inner.topic.clone()
    }

    /// The message payload as raw bytes.
    #[getter]
    fn payload(&self) -> Vec<u8> {
        self.inner.payload.clone()
    }

    /// The QoS level of the message (0, 1, or 2).
    #[getter]
    fn qos(&self) -> u8 {
        self.inner.qos
    }

    /// Whether this is a retained message.
    #[getter]
    fn retain(&self) -> bool {
        self.inner.retain
    }

    /// Decode the payload as UTF-8 text.
    fn text(&self) -> PyResult<String> {
        String::from_utf8(self.inner.payload.clone())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to decode payload as UTF-8: {e}")))
    }

    /// Decode the payload as JSON.
    fn json(&self, py: Python<'_>) -> PyResult<PyObject> {
        let value: serde_json::Value =
            serde_json::from_slice(&self.inner.payload).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to decode payload as JSON: {e}"))
            })?;
        let json_str = serde_json::to_string(&value)
            .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization failed: {e}")))?;
        let json_module = py.import("json")?;
        let result = json_module.call_method1("loads", (json_str,))?;
        Ok(result.into())
    }

    fn __repr__(&self) -> String {
        format!(
            "<MqttMessage topic={:?} qos={}>",
            self.inner.topic, self.inner.qos
        )
    }
}

impl From<RustMqttMessage> for PyMqttMessage {
    fn from(msg: RustMqttMessage) -> Self {
        Self { inner: msg }
    }
}

// ============================================================
// MqttConfig  —  wraps pokecon_core::net::MqttConfig
// ============================================================

/// Configuration for an MQTT client.
///
/// Python usage::
///
///     from pokecon.net import MqttConfig
///     config = MqttConfig(
///         host="localhost",
///         port=1883,
///         client_id="my-client",
///         username=None,
///         password=None,
///         keep_alive=60,
///         clean_session=True,
///     )
#[pyclass(name = "MqttConfig")]
#[derive(Clone)]
pub struct PyMqttConfig {
    inner: RustMqttConfig,
}

#[pymethods]
impl PyMqttConfig {
    /// Create a new MQTT configuration.
    ///
    /// Parameters
    /// ----------
    /// host : str
    ///     MQTT broker hostname or IP (default: ``"localhost"``).
    /// port : int
    ///     MQTT broker port (default: ``1883``).
    /// client_id : str, optional
    ///     Client ID (default: auto-generated).
    /// username : str, optional
    ///     Username for authentication.
    /// password : str, optional
    ///     Password for authentication.
    /// keep_alive : int
    ///     Keep-alive interval in seconds (default: ``60``).
    /// clean_session : bool
    ///     Clean session flag (default: ``True``).
    #[new]
    #[pyo3(signature = (
        host = "localhost".to_string(),
        port = 1883u16,
        client_id = None,
        username = None,
        password = None,
        keep_alive = 60u64,
        clean_session = true,
    ))]
    fn new(
        host: String,
        port: u16,
        client_id: Option<String>,
        username: Option<String>,
        password: Option<String>,
        keep_alive: u64,
        clean_session: bool,
    ) -> Self {
        let inner = RustMqttConfig {
            host,
            port,
            client_id: client_id.unwrap_or_else(|| format!("pokecon-{}", std::process::id())),
            username,
            password,
            keep_alive_secs: keep_alive,
            clean_session,
        };
        Self { inner }
    }

    #[getter]
    fn host(&self) -> String {
        self.inner.host.clone()
    }

    #[getter]
    fn port(&self) -> u16 {
        self.inner.port
    }

    #[getter]
    fn client_id(&self) -> String {
        self.inner.client_id.clone()
    }

    #[getter]
    fn username(&self) -> Option<String> {
        self.inner.username.clone()
    }

    #[getter]
    fn keep_alive(&self) -> u64 {
        self.inner.keep_alive_secs
    }

    #[getter]
    fn clean_session(&self) -> bool {
        self.inner.clean_session
    }

    fn __repr__(&self) -> String {
        format!(
            "<MqttConfig host={:?} port={}>",
            self.inner.host, self.inner.port
        )
    }
}

// ============================================================
// MqttClient  —  wraps pokecon_core::net::MqttClient
// ============================================================

/// MQTT client for publish/subscribe messaging.
///
/// Uses ``rumqttc`` under the hood with automatic reconnection.
///
/// Python usage::
///
///     from pokecon.net import MqttClient, QoS
///
///     # Create and connect
///     client = MqttClient("localhost", 1883)
///     client.connect()
///
///     # Publish a message
///     client.publish("test/topic", b"Hello, MQTT!", qos=QoS.AT_LEAST_ONCE)
///
///     # Subscribe and receive
///     client.subscribe("test/topic")
///     msg = client.recv()
///     print(msg.topic, msg.text())
///
///     # Clean disconnect
///     client.disconnect()
#[pyclass(name = "MqttClient")]
pub struct PyMqttClient {
    inner: RustMqttClient,
}

#[pymethods]
impl PyMqttClient {
    /// Create a new MQTT client.
    ///
    /// Parameters
    /// ----------
    /// host : str
    ///     MQTT broker hostname.
    /// port : int
    ///     MQTT broker port.
    /// client_id : str, optional
    ///     Custom client ID (auto-generated if not provided).
    /// username : str, optional
    ///     Username for broker authentication.
    /// password : str, optional
    ///     Password for broker authentication.
    /// keep_alive : int
    ///     Keep-alive interval in seconds (default: 60).
    /// clean_session : bool
    ///     Clean session flag (default: True).
    #[new]
    #[pyo3(signature = (
        host,
        port,
        client_id = None,
        username = None,
        password = None,
        keep_alive = 60u64,
        clean_session = true,
    ))]
    fn new(
        host: String,
        port: u16,
        client_id: Option<String>,
        username: Option<String>,
        password: Option<String>,
        keep_alive: u64,
        clean_session: bool,
    ) -> Self {
        let config = RustMqttConfig {
            host,
            port,
            client_id: client_id.unwrap_or_else(|| format!("pokecon-{}", std::process::id())),
            username,
            password,
            keep_alive_secs: keep_alive,
            clean_session,
        };
        Self {
            inner: RustMqttClient::new(config),
        }
    }

    /// Connect to the MQTT broker.
    fn connect(&mut self) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.connect())
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT connect failed: {e}")))
    }

    /// Publish a message to a topic.
    ///
    /// Parameters
    /// ----------
    /// topic : str
    ///     The topic to publish to.
    /// payload : bytes
    ///     Message payload.
    /// qos : QoS, optional
    ///     Quality of Service level (default: ``QoS.AT_MOST_ONCE``).
    #[pyo3(signature = (topic, payload, qos = PyQoS::AT_MOST_ONCE))]
    fn publish(&mut self, topic: &str, payload: Vec<u8>, qos: PyQoS) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.publish(topic, payload, qos.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT publish failed: {e}")))
    }

    /// Publish a JSON-serializable value to a topic.
    ///
    /// Parameters
    /// ----------
    /// topic : str
    ///     The topic to publish to.
    /// data : any
    ///     JSON-serializable Python value.
    /// qos : QoS, optional
    ///     Quality of Service level (default: ``QoS.AT_MOST_ONCE``).
    #[pyo3(signature = (topic, data, qos = PyQoS::AT_MOST_ONCE))]
    fn publish_json(&mut self, topic: &str, data: PyObject, qos: PyQoS) -> PyResult<()> {
        // Convert Python object to serde_json::Value via json.dumps + serde_json::from_str
        let json_str = Python::with_gil(|py| -> PyResult<String> {
            let json_module = py.import("json")?;
            let s = json_module.call_method1("dumps", (data,))?;
            s.extract::<String>()
        })?;
        let value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse JSON: {e}")))?;
        let rt = global_runtime();
        rt.block_on(self.inner.publish_json(topic, &value, qos.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT publish_json failed: {e}")))
    }

    /// Publish a retained message to a topic.
    ///
    /// Parameters
    /// ----------
    /// topic : str
    ///     The topic to publish to.
    /// payload : bytes
    ///     Message payload.
    /// qos : QoS, optional
    ///     Quality of Service level (default: ``QoS.AT_MOST_ONCE``).
    #[pyo3(signature = (topic, payload, qos = PyQoS::AT_MOST_ONCE))]
    fn publish_retained(&mut self, topic: &str, payload: Vec<u8>, qos: PyQoS) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.publish_retained(topic, payload, qos.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT publish_retained failed: {e}")))
    }

    /// Subscribe to a topic.
    ///
    /// Parameters
    /// ----------
    /// topic : str
    ///     Topic filter (may include wildcards like ``+`` or ``#``).
    /// qos : QoS, optional
    ///     Quality of Service level (default: ``QoS.AT_MOST_ONCE``).
    #[pyo3(signature = (topic, qos = PyQoS::AT_MOST_ONCE))]
    fn subscribe(&mut self, topic: &str, qos: PyQoS) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.subscribe(topic, qos.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT subscribe failed: {e}")))
    }

    /// Unsubscribe from a topic.
    ///
    /// Parameters
    /// ----------
    /// topic : str
    ///     Topic to unsubscribe from.
    fn unsubscribe(&mut self, topic: &str) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.unsubscribe(topic))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT unsubscribe failed: {e}")))
    }

    /// Try to receive the next message (non-blocking).
    ///
    /// Returns
    /// -------
    /// MqttMessage or None
    ///     The next message, or ``None`` if no message is available.
    fn try_recv(&mut self) -> PyResult<Option<PyMqttMessage>> {
        match self.inner.try_recv() {
            Ok(msg) => Ok(Some(PyMqttMessage::from(msg))),
            Err(_) => Ok(None),
        }
    }

    /// Receive the next message (blocking).
    ///
    /// Parameters
    /// ----------
    /// timeout : float, optional
    ///     Maximum wait time in seconds. If ``None``, waits indefinitely.
    ///
    /// Returns
    /// -------
    /// MqttMessage or None
    ///     The received message, or ``None`` on timeout or if connection ended.
    #[pyo3(signature = (timeout = None))]
    fn recv(&mut self, timeout: Option<f64>) -> PyResult<Option<PyMqttMessage>> {
        let rt = global_runtime();
        let result = if let Some(t) = timeout {
            rt.block_on(async {
                tokio::time::timeout(Duration::from_secs_f64(t), self.inner.recv())
                    .await
                    .ok()
            })
            .and_then(|r| r.ok())
        } else {
            rt.block_on(self.inner.recv()).ok()
        };

        match result {
            Some(msg) => Ok(Some(PyMqttMessage::from(msg))),
            None => Ok(None),
        }
    }

    /// Disconnect from the MQTT broker.
    fn disconnect(&mut self) {
        let rt = global_runtime();
        rt.block_on(self.inner.disconnect());
    }

    /// Check whether the client is connected to the broker.
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Verify the MQTT connection is alive.
    ///
    /// Checks both the event loop health and the last-known connection state.
    ///
    /// Returns
    /// -------
    /// bool
    ///     ``True`` if the event loop is running and the broker acknowledged
    ///     the connection; ``False`` otherwise.
    fn verify_connection(&self) -> PyResult<bool> {
        let rt = global_runtime();
        rt.block_on(self.inner.verify_connection())
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT verify_connection failed: {e}")))
    }

    /// Connect to the MQTT broker with a timeout.
    ///
    /// Waits up to ``timeout_secs`` seconds for the actual MQTT connection
    /// to be established (as confirmed by the broker's CONNACK response).
    ///
    /// Parameters
    /// ----------
    /// timeout_secs : float
    ///     Maximum time to wait for connection in seconds.
    #[pyo3(signature = (timeout_secs))]
    fn connect_with_timeout(&mut self, timeout_secs: f64) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.connect_with_timeout(timeout_secs as u64))
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT connect_with_timeout failed: {e}")))
    }

    /// Attempt to reconnect to the MQTT broker with exponential backoff.
    ///
    /// Disconnects the current session and re-establishes the connection.
    /// Retries up to `max_reconnect_attempts` times with doubling delay.
    fn reconnect(&mut self) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.reconnect())
            .map_err(|e| PyRuntimeError::new_err(format!("MQTT reconnect failed: {e}")))
    }

    /// Set the maximum number of reconnection attempts.
    ///
    /// Parameters
    /// ----------
    /// max : int
    ///     Maximum number of reconnection attempts (default: 5).
    fn set_max_reconnect_attempts(&mut self, max: usize) {
        self.inner.set_max_reconnect_attempts(max);
    }

    /// Get the maximum number of reconnection attempts.
    fn get_max_reconnect_attempts(&self) -> usize {
        self.inner.max_reconnect_attempts()
    }

    /// Set the base delay in seconds for reconnection backoff.
    ///
    /// The actual delay doubles with each failed attempt:
    /// ``delay = base * 2^(attempt - 1)``.
    ///
    /// Parameters
    /// ----------
    /// secs : int
    ///     Base delay in seconds (default: 1).
    fn set_reconnect_base_delay(&mut self, secs: u64) {
        self.inner.set_reconnect_base_delay(secs);
    }

    /// Get the base delay in seconds for reconnection backoff.
    fn get_reconnect_base_delay(&self) -> u64 {
        self.inner.reconnect_base_delay()
    }

    fn __repr__(&self) -> String {
        if self.inner.is_connected() {
            format!(
                "<MqttClient {}:{}>",
                self.inner.config().host,
                self.inner.config().port
            )
        } else {
            "<MqttClient disconnected>".to_string()
        }
    }
}

// ============================================================
// Module registration
// ============================================================

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyHttpClient>()?;
    m.add_class::<PyHttpResponse>()?;
    m.add_class::<PySocketClient>()?;
    m.add_class::<PyWebSocketClient>()?;
    m.add_class::<PyQoS>()?;
    m.add_class::<PyMqttConfig>()?;
    m.add_class::<PyMqttClient>()?;
    m.add_class::<PyMqttMessage>()?;
    Ok(())
}
