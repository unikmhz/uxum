use std::{
    fmt,
    io::{self, Write},
    ops::Not,
    sync::atomic::{AtomicUsize, Ordering},
    thread::JoinHandle,
    time::Duration,
};

use futures::future::pending;
use reqwest::{
    Method,
    header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderValue},
};
use reqwest_middleware::ClientWithMiddleware;
use thread_priority::{ThreadBuilderExt, ThreadPriority};
use tokio::{
    runtime::Builder,
    sync::mpsc::{Receiver, Sender, channel, error::TrySendError},
    time::{MissedTickBehavior, interval},
};
use tokio_util::sync::CancellationToken;
use tracing::subscriber::NoSubscriber;
use url::Url;

use crate::{
    errors::IoError,
    http_client::HttpClientError,
    logging::{LoggingFormat, LoggingHttpCompression, LoggingHttpConfig},
    metrics::MetricsState,
};

/// HTTP log appender error.
#[derive(Debug, thiserror::Error)]
pub enum HttpLogAppenderError {
    /// HTTP client error
    #[error("HTTP client error: {0}")]
    HttpClient(#[from] HttpClientError),
    /// Unable to set up HTTP sender thread
    #[error("Unable to set up HTTP sender thread: {0}")]
    Thread(IoError),
    /// Runtime error for HTTP sender thread
    #[error("Runtime error for HTTP sender thread: {0}")]
    Runtime(IoError),
    #[error("Unable to send logs via HTTP: {0}")]
    Send(#[from] reqwest_middleware::Error),
}

/// Actually write every Nth failure to stderr, to not overwhelm log collectors such as journald
/// with large volume of spurious errors.
const EMIT_NTH_ERROR: u32 = 100;

/// Wrapper for HTTP client for use in sending logs from [`mod@tracing_subscriber::fmt`].
pub struct HttpLogAppender {
    /// Sender end of message queue.
    tx: Sender<HttpLog>,
    /// Signal thread termination.
    token: CancellationToken,
    /// Spawned thread that collects, batches and forwards logs to a remote collector
    /// using HTTP client.
    thread: Option<JoinHandle<Result<(), HttpLogAppenderError>>>,
    /// Count of contiguous errors. This works together with `EMIT_NTH_ERROR` constant to
    /// rate-limit emission of internal errors.
    err_count: u32,
    /// Return errors to the logger when internal queue is full or closed.
    back_pressure: bool,
}

/// Parameters passed into log sender thread.
struct HttpLogAppenderThread {
    /// Receiver end of message queue.
    rx: Receiver<HttpLog>,
    /// Signal thread termination.
    token: CancellationToken,
    /// HTTP client to use when calling remote log collector endpoint.
    client: ClientWithMiddleware,
    /// URL to send aggregated logs to.
    endpoint: Url,
    /// HTTP method to use when calling [`Self::endpoint`].
    method: Method,
    /// Content-type header value to use when calling [`Self::endpoint`].
    content_type: HeaderValue,
    /// Max count of log messages to batch together and send in a single request.
    max_batch_size: usize,
    /// Do not disable tracing spans and events inside HTTP log appender thread if true.
    debug: bool,
    /// Do not initiate new HTTP request on receiving flush operation.
    ignore_flush: bool,
    /// Enable optional periodic forced flush of accumulated logs.
    periodic_flush: Option<Duration>,
    /// Compression to apply to request bodies.
    compression: Option<LoggingHttpCompression>,
}

/// Type which inhabits HTTP log queue.
enum HttpLog {
    /// Flush accumulated buffer.
    Flush,
    /// Send log message to buffer.
    Message {
        /// Raw (albeit formatted) log message data.
        data: Vec<u8>,
    },
}

impl fmt::Display for HttpLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flush => write!(f, "log flush request"),
            Self::Message { data } => write!(
                f,
                "log message: {}",
                str::from_utf8(data).unwrap_or("invalid UTF-8 in message")
            ),
        }
    }
}

impl HttpLogAppender {
    /// Creates new appender object.
    ///
    /// # Errors
    ///
    /// Returns `Err` if an error was encountered while creating and initializing a Kafka producer.
    pub async fn new(
        config: &LoggingHttpConfig,
        format: &LoggingFormat,
        metrics: Option<&MetricsState>,
    ) -> Result<Self, HttpLogAppenderError> {
        static THREAD_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let token = CancellationToken::new();
        let (tx, rx) = channel::<HttpLog>(config.queue.get());
        let content_type = match format {
            LoggingFormat::Json { .. } => HeaderValue::from_static("application/jsonl"),
            _ => HeaderValue::from_static("text/plain"),
        };
        let metrics = metrics.map(|m| m.client_metrics(&config.http_client_name));
        let client = config.http_client.to_client(metrics).await?;
        let method = config.method.into();
        let thread_params = HttpLogAppenderThread {
            rx,
            token: token.clone(),
            client,
            endpoint: config.endpoint.clone(),
            method,
            content_type,
            max_batch_size: config.max_batch_size.get(),
            debug: config.debug,
            ignore_flush: config.ignore_flush,
            periodic_flush: config.periodic_flush,
            compression: config.compression,
        };
        let thread_num = THREAD_COUNTER.fetch_and(1, Ordering::Relaxed);
        let writer_thread = std::thread::Builder::new()
            .name(format!("http-logger-{thread_num}"))
            .spawn_with_priority(ThreadPriority::Min, move |setprio| {
                if let Err(err) = setprio {
                    eprintln!("Unable to set thread priority for HTTP sender thread: {err}");
                }
                http_logger_thread(thread_params)
            })
            .map_err(|err| HttpLogAppenderError::Thread(err.into()))?;
        Ok(Self {
            tx,
            token,
            thread: Some(writer_thread),
            err_count: 0,
            back_pressure: config.back_pressure,
        })
    }

    #[inline]
    fn alert_stderr(&mut self, err: &TrySendError<HttpLog>) {
        if self.err_count == 0 {
            match err {
                TrySendError::Full(inner) => eprintln!("HTTP log queue full: {inner}"),
                TrySendError::Closed(inner) => eprintln!("HTTP log queue closed: {inner}"),
            }
        }
        self.err_count += 1;
        if self.err_count == EMIT_NTH_ERROR {
            self.err_count = 0;
        }
    }

    #[inline]
    fn reset_stderr(&mut self) {
        self.err_count = 0;
    }
}

impl Drop for HttpLogAppender {
    fn drop(&mut self) {
        self.token.cancel();
        if let Some(thread) = self.thread.take() {
            match thread.join() {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => eprintln!("Unable to shut down HTTP log appender thread: {err}"),
                Err(_) => eprintln!("HTTP log appender thread panic detected"),
            }
        }
    }
}

impl Write for HttpLogAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Err(err) = self.tx.try_send(HttpLog::Message { data: buf.to_vec() }) {
            self.alert_stderr(&err);
            if self.back_pressure {
                return Err(io::Error::other(err));
            } else {
                return Ok(buf.len());
            }
        }
        self.reset_stderr();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Err(err) = self.tx.try_send(HttpLog::Flush) {
            self.alert_stderr(&err);
            if self.back_pressure {
                return Err(io::Error::other(err));
            } else {
                return Ok(());
            }
        }
        self.reset_stderr();
        Ok(())
    }
}

fn http_logger_thread(params: HttpLogAppenderThread) -> Result<(), HttpLogAppenderError> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| HttpLogAppenderError::Runtime(err.into()))?
        .block_on(http_logger_task(params))
}

async fn http_logger_task(mut params: HttpLogAppenderThread) -> Result<(), HttpLogAppenderError> {
    let _guard = params
        .debug
        .not()
        .then(|| tracing::subscriber::set_default(NoSubscriber::new()));
    let mut buffered = Vec::with_capacity(params.max_batch_size);
    let mut interval = params.periodic_flush.map(|dur| {
        let mut interval = interval(dur);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    });
    loop {
        tokio::select! {
            _ = params.token.cancelled() => {
                break;
            }
            _ = async {
                match &mut interval {
                    Some(i) => i.tick().await,
                    None => pending().await,
                }
            } => {
                if !buffered.is_empty() {
                    send_logs(&params, &buffered).await;
                    buffered.clear();
                }
            }
            msg = params.rx.recv() => match msg {
                Some(msg) => match msg {
                    HttpLog::Flush => {
                        if !params.ignore_flush && !buffered.is_empty() {
                            send_logs(&params, &buffered).await;
                            buffered.clear();
                        }
                    }
                    HttpLog::Message { data } => {
                        buffered.push(data);
                        if buffered.len() >= params.max_batch_size {
                            send_logs(&params, &buffered).await;
                            buffered.clear();
                        }
                    }
                }
                None => break,
            }
        }
    }
    // Slurping leftovers and sending them if need be.
    while let Ok(msg) = params.rx.try_recv() {
        if let HttpLog::Message { data } = msg {
            buffered.push(data);
        }
    }
    if !buffered.is_empty() {
        for chunk in buffered.chunks(params.max_batch_size) {
            send_logs(&params, chunk).await;
        }
    }
    Ok(())
}

async fn send_logs(params: &HttpLogAppenderThread, logs: &[Vec<u8>]) {
    let mut body = Vec::with_capacity(logs.iter().map(|line| line.len()).sum());
    for line in logs {
        body.extend_from_slice(line.as_slice());
    }

    let (body, encoding_header) = compress_body(body, params.compression);
    let mut request = params
        .client
        .request(params.method.clone(), params.endpoint.clone())
        .header(CONTENT_TYPE, &params.content_type);

    // if `encoding_header` contains a value here - the body has been
    // compressed ok previously
    if let Some(enc_hdr) = encoding_header {
        request = request.header(CONTENT_ENCODING, enc_hdr);
    }

    let resp = match request.body(body).send().await {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Unable to send HTTP logs: {err}");
            return;
        }
    };
    if let Err(err) = resp.error_for_status() {
        eprintln!("Error response received after sending HTTP logs: {err}");
    };
}

/// Compress request body if compression is configured.
///
/// Returns the (possibly compressed) body and an appropriate value for
/// 'content-encoding' header.
/// If the compression was not requested OR was requested but failed
/// - the original request body is returned with `None` value for header.
fn compress_body(
    body: Vec<u8>,
    compression: Option<LoggingHttpCompression>,
) -> (Vec<u8>, Option<HeaderValue>) {
    let Some(compression) = compression else {
        return (body, None);
    };
    match compression {
        #[cfg(feature = "http-log-gzip")]
        LoggingHttpCompression::Gzip => {
            use flate2::{Compression, write::GzEncoder};
            use std::io::Write as _;

            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            if encoder.write_all(&body).is_ok() {
                if let Ok(compressed) = encoder.finish() {
                    return (compressed, HeaderValue::from_static("gzip"));
                }
            }
            eprintln!("HTTP log appender: gzip compression failed, sending uncompressed");
            (body, None)
        }
        #[cfg(feature = "http-log-zstd")]
        LoggingHttpCompression::Zstd => {
            use std::io::Write as _;
            use zstd::stream::write::Encoder;

            match Encoder::new(Vec::new(), 0) {
                Ok(mut encoder) => {
                    if encoder.write_all(&body).is_ok() {
                        if let Ok(compressed) = encoder.finish() {
                            return (compressed, HeaderValue::from_static("zstd"));
                        }
                    }
                    eprintln!("HTTP log appender: zstd compression failed, sending uncompressed");
                    (body, None)
                }
                Err(_) => {
                    eprintln!("HTTP log appender: zstd encoder init failed, sending uncompressed");
                    (body, None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_compression_returns_body_unchanged() {
        let body = b"line1\nline2\n".to_vec();
        let (out, hdr) = compress_body(body.clone(), None);
        assert!(hdr.is_none());
        assert_eq!(out, body);
    }

    #[cfg(feature = "http-log-gzip")]
    #[test]
    fn gzip_roundtrip() {
        let body = b"{\"level\":\"INFO\"}\n{\"level\":\"ERROR\"}\n".to_vec();
        let (compressed, hdr) = compress_body(body.clone(), Some(LoggingHttpCompression::Gzip));
        assert_eq!(
            hdr.is_some(),
            "content-encoding header is None for successfully compressed body"
        );
        assert_eq!(hdr.unwrap(), HeaderValue::from_static("gzip"));
        assert_ne!(compressed, body);

        use std::io::Read as _;
        let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .expect("valid gzip stream");
        assert_eq!(decoded, body);
    }

    #[cfg(feature = "http-log-zstd")]
    #[test]
    fn zstd_roundtrip() {
        let body = b"{\"level\":\"INFO\"}\n{\"level\":\"ERROR\"}\n".to_vec();
        let (compressed, hdr) = compress_body(body.clone(), Some(LoggingHttpCompression::Zstd));
        assert_eq!(
            hdr.is_some(),
            "content-encoding header is None for successfully compressed body"
        );
        assert_eq!(hdr.unwrap(), HeaderValue::from_static("zstd"));

        use std::io::Read as _;
        let mut decoder =
            zstd::stream::read::Decoder::new(&compressed[..]).expect("zstd decoder init");
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .expect("valid zstd stream");
        assert_eq!(decoded, body);
    }

    #[cfg(feature = "http-log-gzip")]
    #[test]
    fn http_config_parses_compression_gzip() {
        #[derive(serde::Deserialize)]
        struct TestCfg {
            compression: Option<LoggingHttpCompression>,
        }
        let cfg: TestCfg = serde_json::from_str(r#"{"compression": "gzip"}"#).expect("valid json");
        assert_eq!(cfg.compression, Some(LoggingHttpCompression::Gzip));
    }

    #[cfg(feature = "http-log-zstd")]
    #[test]
    fn http_config_parses_compression_zstd() {
        #[derive(serde::Deserialize)]
        struct TestCfg {
            compression: Option<LoggingHttpCompression>,
        }
        let cfg: TestCfg = serde_json::from_str(r#"{"compression": "zstd"}"#).expect("valid json");
        assert_eq!(cfg.compression, Some(LoggingHttpCompression::Zstd));
    }
}
