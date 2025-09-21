//! Kafka configuration and utility functions.

use std::{collections::HashMap, time::Duration};

use rdkafka::{
    client::ClientContext,
    config::ClientConfig,
    error::{KafkaError, KafkaResult},
    message::{BorrowedMessage, Message},
    producer::{BaseRecord, NoCustomPartitioner, Producer, ProducerContext, ThreadedProducer},
};
use serde::{Deserialize, Serialize};

/// Kafka producer configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaProducerConfig {
    /// Configuration common to both producers and consumers.
    #[serde(flatten)]
    pub common: KafkaCommonConfig,
    /// Transactional mode configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<KafkaTransactionalConfig>,
    /// In-process producer queue configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<KafkaQueueConfig>,
    /// Max number of tries when sending a message.
    ///
    /// See `message.send.max.retries` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_retries: Option<i32>,
    /// Target topic to write to.
    pub topic: String,
    /// Enable at-most-once mode.
    ///
    /// See `enable.idempotence` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default = "crate::util::default_true")]
    pub idempotence: bool,
    /// Enable erroring out when errors would create a gap in produced batch.
    ///
    /// See `enable.gapless.guarantee` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default)]
    pub gapless: bool,
    /// Number of acknowledgements a leader broker must receive from its peers. `-1` means all.
    /// `0` disables acknowledgements altogether.
    ///
    /// See `request.required.acks` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default = "KafkaProducerConfig::default_acks")]
    pub acks: i16,
    /// Local producer message delivery timeout.
    ///
    /// See `message.timeout.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub message_timeout: Option<Duration>,
    /// ACK timeout of producer request.
    ///
    /// See `request.timeout.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub request_timeout: Option<Duration>,
    /// Timeout to use when calling [`ThreadedProducer::flush`].
    #[serde(
        default = "KafkaProducerConfig::default_flush_timeout",
        with = "humantime_serde"
    )]
    pub flush_timeout: Duration,
    /// Time to wait for messages to arrive in local queue before dispatching them in bulk.
    ///
    /// See `queue.buffering.max.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub linger: Option<Duration>,
}

impl KafkaProducerConfig {
    /// Default value for [`Self::acks`].
    #[must_use]
    #[inline]
    fn default_acks() -> i16 {
        -1
    }

    /// Default value for [`Self::flush_timeout`].
    #[must_use]
    #[inline]
    fn default_flush_timeout() -> Duration {
        Duration::from_secs(5)
    }

    /// Create a [`ClientConfig`] from this configuration.
    pub fn make_client_config(&self) -> ClientConfig {
        let mut client = ClientConfig::new();
        self.common.configure(&mut client);
        client.set("enable.idempotence", kafka_config_bool(self.idempotence));
        client.set("enable.gapless.guarantee", kafka_config_bool(self.gapless));
        client.set("request.required.acks", self.acks.to_string());
        if let Some(msg_timeout) = self.message_timeout {
            client.set("message.timeout.ms", kafka_ms(msg_timeout));
        }
        if let Some(req_timeout) = self.request_timeout {
            client.set("request.timeout.ms", kafka_ms(req_timeout));
        }
        if let Some(linger) = self.linger {
            client.set("queue.buffering.max.ms", kafka_ms(linger));
        }
        if let Some(cfg) = &self.transaction {
            cfg.configure(&mut client);
        }
        if let Some(cfg) = &self.queue {
            cfg.configure(&mut client);
        }
        if let Some(retries) = self.max_retries {
            client.set("message.send.max.retries", retries.to_string());
        }
        client
    }

    /// Create a [`ThreadedProducer`] from this configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an error was encountered while creating and initializing a Kafka producer.
    pub fn make_sync_producer<C: ProducerContext>(
        &self,
        context: C,
    ) -> KafkaResult<ThreadedProducer<C>> {
        let client = self.make_client_config();
        let producer: ThreadedProducer<C> = client.create_with_context(context)?;
        if let Some(trxn) = &self.transaction {
            producer.init_transactions(trxn.init_timeout)?;
        }
        Ok(producer)
    }
}

/// Common Kafka configuration for both producers and consumers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaCommonConfig {
    /// List of brokers to connect to. Each element needs to be in `<host>` or `<host>:<port>`
    /// format.
    ///
    /// See `bootstrap.servers` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brokers: Vec<String>,
    /// Authentication-related properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<KafkaAuthConfig>,
    /// Retry backoff properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff: Option<KafkaBackoffConfig>,
    /// Additional parameters to include in Kafka configuration.
    #[serde(
        default,
        alias = "rdkafka_params",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub extra_params: HashMap<String, String>,
}

impl KafkaCommonConfig {
    /// Apply relevant configuration to [`ClientConfig`].
    pub fn configure(&self, client: &mut ClientConfig) {
        client.set("bootstrap.servers", self.brokers.join(","));
        if let Some(cfg) = &self.auth {
            cfg.configure(client);
        }
        if let Some(cfg) = &self.backoff {
            cfg.configure(client);
        }
        for (key, value) in &self.extra_params {
            client.set(key, value);
        }
    }
}

/// Authentication-related Kafka properties.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaAuthConfig {
    /// SASL username to use. Applicable to `PLAIN` and `SCRAM-...` mechanisms.
    ///
    /// See `sasl.username` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    pub username: Option<String>,
    /// SASL password to use. Applicable to `PLAIN` and `SCRAM-...` mechanisms.
    ///
    /// See `sasl.password` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    pub password: Option<String>, // TODO: uxum-passwd
    /// Kafka security protocol to use.
    ///
    /// See `security.protocol` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default)]
    pub protocol: KafkaProtocol,
    /// Kafka authentication mechanism to use.
    ///
    /// See `sasl.mechanisms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default)]
    pub mechanism: KafkaAuthMechanism,
}

impl KafkaAuthConfig {
    /// Apply relevant configuration to [`ClientConfig`].
    pub fn configure(&self, client: &mut ClientConfig) {
        if let Some(username) = &self.username {
            client.set("sasl.username", username);
        }
        if let Some(password) = &self.password {
            client.set("sasl.password", password);
        }
        client.set("sasl.mechanisms", self.mechanism.as_ref());
        client.set("security.protocol", self.protocol.as_ref());
    }
}

/// Kafka security protocol to use.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum KafkaProtocol {
    /// Without authentication, without encryption.
    Plaintext,
    /// Without authentication, with encryption.
    Ssl,
    /// With authentication, without encryption.
    SaslPlaintext,
    /// With authentication, with encryption.
    #[default]
    SaslSsl,
}

impl AsRef<str> for KafkaProtocol {
    fn as_ref(&self) -> &str {
        match self {
            Self::Plaintext => "plaintext",
            Self::Ssl => "ssl",
            Self::SaslPlaintext => "sasl_plaintext",
            Self::SaslSsl => "sasl_ssl",
        }
    }
}

/// Kafka authentication mechanism to use.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "UPPERCASE")]
pub enum KafkaAuthMechanism {
    /// Authenticate via GSSAPI (typically Kerberos).
    Gssapi,
    /// Authenticate via plain username and password.
    Plain,
    /// Authenticate via SCRAM algorithm using SHA-256 hashes.
    #[serde(rename = "SCRAM-SHA-256", alias = "SCRAM-SHA256")]
    ScramSha256,
    /// Authenticate via SCRAM algorithm using SHA-512 hashes.
    #[serde(rename = "SCRAM-SHA-512", alias = "SCRAM-SHA512")]
    #[default]
    ScramSha512,
    /// Authenticate via OAuth Bearer. Additional configuration is required.
    #[serde(alias = "OAUTH-BEARER")]
    OAuthBearer,
}

impl AsRef<str> for KafkaAuthMechanism {
    fn as_ref(&self) -> &str {
        match self {
            Self::Gssapi => "GSSAPI",
            Self::Plain => "PLAIN",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::ScramSha512 => "SCRAM-SHA-512",
            Self::OAuthBearer => "OAUTHBEARER",
        }
    }
}

/// Transaction configuration for Kafka producers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaTransactionalConfig {
    /// ID string for transactional producer.
    ///
    /// See `transactional.id` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    pub id: String,
    /// Maximum time for a transaction to exist.
    ///
    /// See `transaction.timeout.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub timeout: Option<Duration>,
    /// Maximum time to wait for transaction initialization.
    #[serde(
        default = "KafkaTransactionalConfig::default_init_timeout",
        with = "humantime_serde"
    )]
    pub init_timeout: Duration,
}

impl KafkaTransactionalConfig {
    /// Default value for [`Self::init_timeout`].
    #[must_use]
    #[inline]
    fn default_init_timeout() -> Duration {
        Duration::from_secs(5)
    }

    /// Apply relevant configuration to [`ClientConfig`].
    pub fn configure(&self, client: &mut ClientConfig) {
        client.set("transactional.id", &self.id);
        if let Some(timeout) = self.timeout {
            client.set("transaction.timeout.ms", kafka_ms(timeout));
        }
    }
}

/// Kafka local producer queue configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaQueueConfig {
    /// Maximum number of messages to keep in the queue.
    ///
    /// See `queue.buffering.max.messages` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<i32>,
    /// Maximum total message size to keep in the queue.
    ///
    /// See `queue.buffering.max.kbytes` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_kbytes: Option<i32>,
    /// Maximum time for message to wait inside a queue. Used to optimize sending messages in
    /// batches.
    ///
    /// See `queue.buffering.max.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub max_duration: Option<Duration>,
}

impl KafkaQueueConfig {
    /// Apply relevant configuration to [`ClientConfig`].
    pub fn configure(&self, client: &mut ClientConfig) {
        if let Some(max_msg) = self.max_messages {
            client.set("queue.buffering.max.messages", max_msg.to_string());
        }
        if let Some(max_kb) = self.max_kbytes {
            client.set("queue.buffering.max.kbytes", max_kb.to_string());
        }
        if let Some(max_dur) = self.max_duration {
            client.set("queue.buffering.max.ms", kafka_ms(max_dur));
        }
    }
}

/// Kafka backoff time configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct KafkaBackoffConfig {
    /// Minimum backoff wait time before retrying a protocol request.
    ///
    /// See `retry.backoff.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    min: Option<Duration>,
    /// Maximum backoff wait time before retrying a protocol request.
    ///
    /// See `retry.backoff.max.ms` parameter from [librdkafka].
    ///
    /// [librdkafka]: https://docs.confluent.io/platform/current/clients/librdkafka/html/md_CONFIGURATION.html
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    max: Option<Duration>,
}

impl KafkaBackoffConfig {
    /// Apply relevant configuration to [`ClientConfig`].
    pub fn configure(&self, client: &mut ClientConfig) {
        if let Some(min) = self.min {
            client.set("retry.backoff.ms", kafka_ms(min));
        }
        if let Some(max) = self.max {
            client.set("retry.backoff.max.ms", kafka_ms(max));
        }
    }
}

/// Convert boolean value for use in [`ClientConfig`].
fn kafka_config_bool(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

/// Format duration as number of milliseconds, for use in Kafka configuration.
fn kafka_ms(d: Duration) -> String {
    d.as_millis().to_string()
}

/// Specialized producer context to use when writing logs to Kafka topics.
pub struct LogProducerContext;

impl ClientContext for LogProducerContext {}

impl ProducerContext<NoCustomPartitioner> for LogProducerContext {
    type DeliveryOpaque = ();

    fn delivery(
        &self,
        res: &Result<BorrowedMessage<'_>, (KafkaError, BorrowedMessage<'_>)>,
        _: <Self as ProducerContext>::DeliveryOpaque,
    ) {
        if let Err((err, msg)) = res {
            eprintln!(
                "Kafka log appender error (topic:{} partition:{} offset:{}): {err}",
                msg.topic(),
                msg.partition(),
                msg.offset(),
            );
        }
    }
}

/// Wrapper for Kafka producer for use in writing logs from [`tracing_subscriber::fmt`].
pub struct KafkaLogAppender {
    config: KafkaProducerConfig,
    producer: ThreadedProducer<LogProducerContext>,
}

impl KafkaLogAppender {
    /// Creates new appender object.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an error was encountered while creating and initializing a Kafka producer.
    pub fn new(config: &KafkaProducerConfig) -> KafkaResult<Self> {
        let producer = config.make_sync_producer(LogProducerContext)?;
        Ok(KafkaLogAppender {
            config: config.clone(),
            producer,
        })
    }
}

impl std::io::Write for KafkaLogAppender {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // TODO: detect newlines if needed, possibly splitting into multiple messages.
        let record: BaseRecord<'_, (), _> = BaseRecord::to(&self.config.topic).payload(buf);
        self.producer
            .send(record)
            .map_err(|(err, _)| std::io::Error::other(err))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.producer
            .flush(self.config.flush_timeout)
            .map_err(std::io::Error::other)
    }
}
