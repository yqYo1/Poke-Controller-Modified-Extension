use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, Publish, QoS};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// MQTT communication error types
#[derive(Error, Debug)]
pub enum MqttError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Failed to publish to topic '{topic}': {detail}")]
    PublishError { topic: String, detail: String },

    #[error("Failed to subscribe to topic '{topic}': {detail}")]
    SubscribeError { topic: String, detail: String },

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Not connected to MQTT broker")]
    NotConnected,

    #[error("Event loop ended")]
    EventLoopEnded,
}

/// Represents an incoming MQTT message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttMessage {
    /// The topic the message was received on
    pub topic: String,
    /// The message payload as raw bytes
    pub payload: Vec<u8>,
    /// The QoS level of the message
    pub qos: u8,
    /// Whether this is a retained message
    pub retain: bool,
}

impl From<Publish> for MqttMessage {
    fn from(publish: Publish) -> Self {
        Self {
            topic: publish.topic,
            payload: publish.payload.to_vec(),
            qos: publish.qos as u8,
            retain: publish.retain,
        }
    }
}

/// Configuration for the MQTT client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    /// MQTT broker hostname or IP
    pub host: String,
    /// MQTT broker port
    pub port: u16,
    /// Client ID for the MQTT connection
    pub client_id: String,
    /// Username for authentication (optional)
    pub username: Option<String>,
    /// Password for authentication (optional)
    pub password: Option<String>,
    /// Keep-alive interval in seconds
    pub keep_alive_secs: u64,
    /// Clean session flag
    pub clean_session: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1883,
            client_id: format!("pokecon-{}", std::process::id()),
            username: None,
            password: None,
            keep_alive_secs: 60,
            clean_session: true,
        }
    }
}

/// An MQTT client using `rumqttc`
///
/// Provides async connect, publish, subscribe, and message receiving.
pub struct MqttClient {
    /// The async MQTT client from rumqttc
    client: Option<AsyncClient>,
    /// Receiver for incoming messages
    message_rx: Option<mpsc::Receiver<MqttMessage>>,
    /// The MQTT configuration
    config: MqttConfig,
    /// Whether the client is connected
    connected: bool,
    /// Handle to the event loop task
    event_loop_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MqttClient {
    /// Create a new MQTT client with the given configuration
    pub fn new(config: MqttConfig) -> Self {
        Self {
            client: None,
            message_rx: None,
            config,
            connected: false,
            event_loop_handle: None,
        }
    }

    /// Create a new MQTT client with default configuration
    pub fn default_with_host(host: &str, port: u16) -> Self {
        let config = MqttConfig {
            host: host.to_string(),
            port,
            ..Default::default()
        };
        Self::new(config)
    }

    /// Connect to the MQTT broker
    pub async fn connect(&mut self) -> Result<(), MqttError> {
        info!(
            "Connecting to MQTT broker at {}:{}",
            self.config.host, self.config.port
        );

        let mut mqtt_options =
            MqttOptions::new(&self.config.client_id, &self.config.host, self.config.port);
        mqtt_options.set_keep_alive(Duration::from_secs(self.config.keep_alive_secs));
        mqtt_options.set_clean_session(self.config.clean_session);

        if let Some(ref username) = self.config.username {
            let password = self.config.password.as_deref().unwrap_or("");
            mqtt_options.set_credentials(username, password);
        }

        let (client, mut event_loop) = AsyncClient::new(mqtt_options, 100);

        // Channel for receiving decoded messages
        let (tx, rx) = mpsc::channel(256);
        self.message_rx = Some(rx);

        // Spawn the event loop task
        let handle = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let msg = MqttMessage::from(publish);
                        if tx.send(msg).await.is_err() {
                            warn!("Message channel closed, stopping event loop");
                            break;
                        }
                    }
                    Ok(Event::Incoming(packet)) => {
                        debug!("Received MQTT packet: {:?}", packet);
                    }
                    Ok(Event::Outgoing(outgoing)) => {
                        debug!("MQTT outgoing: {:?}", outgoing);
                    }
                    Err(e) => {
                        error!("MQTT event loop error: {:?}", e);
                        // Don't break on temporary errors — let rumqttc reconnect
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        self.client = Some(client);
        self.event_loop_handle = Some(handle);
        self.connected = true;

        info!(
            "Connected to MQTT broker at {}:{}",
            self.config.host, self.config.port
        );
        Ok(())
    }

    /// Publish a message to a topic with the given QoS
    pub async fn publish(
        &mut self,
        topic: &str,
        payload: impl Into<Vec<u8>>,
        qos: QoS,
    ) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::NotConnected)?;

        let payload_bytes: Vec<u8> = payload.into();
        debug!(
            "Publishing {} bytes to topic: {}",
            payload_bytes.len(),
            topic
        );

        client
            .publish(topic, qos, false, payload_bytes)
            .await
            .map_err(|e| MqttError::PublishError {
                topic: topic.to_string(),
                detail: e.to_string(),
            })?;

        Ok(())
    }

    /// Publish a JSON-serializable message to a topic
    pub async fn publish_json<T: Serialize>(
        &mut self,
        topic: &str,
        data: &T,
        qos: QoS,
    ) -> Result<(), MqttError> {
        let payload = serde_json::to_vec(data).map_err(|e| {
            MqttError::SerializationError(format!("Failed to serialize to JSON: {}", e))
        })?;
        self.publish(topic, payload, qos).await
    }

    /// Publish a retained message to a topic
    pub async fn publish_retained(
        &mut self,
        topic: &str,
        payload: impl Into<Vec<u8>>,
        qos: QoS,
    ) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::NotConnected)?;

        let payload_bytes: Vec<u8> = payload.into();
        client
            .publish(topic, qos, true, payload_bytes)
            .await
            .map_err(|e| MqttError::PublishError {
                topic: topic.to_string(),
                detail: e.to_string(),
            })?;

        Ok(())
    }

    /// Subscribe to a topic with the given QoS
    pub async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::NotConnected)?;

        info!("Subscribing to topic: {} (QoS {:?})", topic, qos);
        client
            .subscribe(topic, qos)
            .await
            .map_err(|e| MqttError::SubscribeError {
                topic: topic.to_string(),
                detail: e.to_string(),
            })?;

        Ok(())
    }

    /// Subscribe to multiple topics with the same QoS
    pub async fn subscribe_many(&mut self, topics: &[(&str, QoS)]) -> Result<(), MqttError> {
        for (topic, qos) in topics {
            self.subscribe(topic, *qos).await?;
        }
        Ok(())
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&mut self, topic: &str) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::NotConnected)?;

        info!("Unsubscribing from topic: {}", topic);
        client
            .unsubscribe(topic)
            .await
            .map_err(|e| MqttError::SubscribeError {
                topic: topic.to_string(),
                detail: e.to_string(),
            })?;

        Ok(())
    }

    /// Try to receive the next message (non-blocking)
    pub fn try_recv(&mut self) -> Result<MqttMessage, MqttError> {
        let rx = self.message_rx.as_mut().ok_or(MqttError::NotConnected)?;
        rx.try_recv().map_err(|_| MqttError::EventLoopEnded)
    }

    /// Receive the next message (blocking/async)
    pub async fn recv(&mut self) -> Result<MqttMessage, MqttError> {
        let rx = self.message_rx.as_mut().ok_or(MqttError::NotConnected)?;
        rx.recv().await.ok_or(MqttError::EventLoopEnded)
    }

    /// Disconnect from the MQTT broker
    pub async fn disconnect(&mut self) {
        info!("Disconnecting from MQTT broker");

        if let Some(client) = self.client.take() {
            // Drop the client; the event loop will exit on its own
            drop(client);
        }

        if let Some(handle) = self.event_loop_handle.take() {
            handle.abort();
        }

        self.connected = false;
        self.message_rx = None;
        info!("Disconnected from MQTT broker");
    }

    /// Check if the client is connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the current configuration
    pub fn config(&self) -> &MqttConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test MQTT config creation with defaults
    #[test]
    fn test_mqtt_config_default() {
        let config = MqttConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.keep_alive_secs, 60);
        assert!(config.clean_session);
    }

    /// Test MqttMessage conversion from rumqttc Publish
    #[test]
    fn test_mqtt_message_from_publish() {
        let publish = Publish::new("test/topic", QoS::AtLeastOnce, b"hello payload");
        let msg = MqttMessage::from(publish);

        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.payload, b"hello payload");
        assert_eq!(msg.qos, QoS::AtLeastOnce as u8);
        assert!(!msg.retain);
    }

    /// Test MqttMessage serde roundtrip
    #[test]
    fn test_mqtt_message_serde() {
        let msg = MqttMessage {
            topic: "test/topic".to_string(),
            payload: vec![1, 2, 3, 4],
            qos: 1,
            retain: false,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: MqttMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.topic, msg.topic);
        assert_eq!(deserialized.payload, msg.payload);
        assert_eq!(deserialized.qos, msg.qos);
        assert_eq!(deserialized.retain, msg.retain);
    }

    /// Test MqttClient creation
    #[test]
    fn test_mqtt_client_new() {
        let config = MqttConfig::default();
        let client = MqttClient::new(config);
        assert!(!client.is_connected());
    }

    /// Test default_with_host
    #[test]
    fn test_mqtt_client_default_with_host() {
        let client = MqttClient::default_with_host("mqtt.example.com", 1883);
        assert_eq!(client.config().host, "mqtt.example.com");
        assert_eq!(client.config().port, 1883);
        assert!(!client.is_connected());
    }

    /// Test error when publishing without connection
    #[tokio::test]
    async fn test_publish_without_connect() {
        let config = MqttConfig::default();
        let mut client = MqttClient::new(config);
        let result = client.publish("test", b"data", QoS::AtMostOnce).await;
        assert!(matches!(result, Err(MqttError::NotConnected)));
    }

    /// Test error when subscribing without connection
    #[tokio::test]
    async fn test_subscribe_without_connect() {
        let config = MqttConfig::default();
        let mut client = MqttClient::new(config);
        let result = client.subscribe("test", QoS::AtMostOnce).await;
        assert!(matches!(result, Err(MqttError::NotConnected)));
    }
}
