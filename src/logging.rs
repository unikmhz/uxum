use std::{fs, io};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_appender::{
    non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    filter::LevelFilter,
    fmt::{self, writer::BoxMakeWriter},
    layer::{Layer, Layered, SubscriberExt},
    registry::Registry,
};

type LoggingRegistry = Layered<Vec<Box<dyn Layer<Registry> + Send + Sync>>, Registry>;

/// Error type used in logging configuration
#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("Log destination I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Error while initializing log directory writer: {0}")]
    Directory(#[from] tracing_appender::rolling::InitError),
}

/// Logging configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub subscribers: Vec<LoggingSubscriberConfig>,
}

impl LoggingConfig {
    ///
    pub fn make_registry(&self) -> Result<(LoggingRegistry, Vec<WorkerGuard>), LoggingError> {
        let num_subs = self.subscribers.len();
        let (subs, buf_guards) = self.subscribers.iter().try_fold(
            (Vec::with_capacity(num_subs), Vec::with_capacity(num_subs)),
            |(mut acc_s, mut acc_g), sub_cfg| {
                let (sub, guard) = sub_cfg.make_layer()?;
                acc_s.push(sub);
                acc_g.push(guard);
                Ok::<_, LoggingError>((acc_s, acc_g))
            },
        )?;
        Ok((Registry::default().with(subs), buf_guards))
    }
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LoggingSubscriberConfig {
    ///
    #[serde(default, flatten)]
    pub format: LoggingFormat,
    ///
    #[serde(default)]
    pub level: LoggingLevel,
    /// Use ANSI escape sequences for output colors and formatting
    #[serde(default)]
    pub color: bool,
    ///
    #[serde(default = "crate::util::default_true")]
    pub internal_errors: bool,
    ///
    #[serde(default)]
    pub print: LoggingPrintingConfig,
    ///
    #[serde(default)]
    pub buffer: LoggingBufferConfig,
    ///
    #[serde(default)]
    pub output: LoggingDestination,
}

impl Default for LoggingSubscriberConfig {
    fn default() -> Self {
        Self {
            format: LoggingFormat::Full,
            level: LoggingLevel::Debug,
            color: false,
            internal_errors: true,
            print: LoggingPrintingConfig::default(),
            buffer: LoggingBufferConfig::default(),
            output: LoggingDestination::default(),
        }
    }
}

impl LoggingSubscriberConfig {
    ///
    #[must_use]
    pub fn default_for_dev() -> Self {
        Self {
            format: LoggingFormat::Pretty,
            level: LoggingLevel::Trace,
            color: true,
            internal_errors: true,
            print: LoggingPrintingConfig {
                target: true,
                file: true,
                line_number: true,
                level: true,
                thread_name: true,
                thread_id: false,
            },
            buffer: LoggingBufferConfig::default(),
            output: LoggingDestination::default(),
        }
    }

    ///
    pub fn make_layer<T>(
        &self,
    ) -> Result<(Box<dyn Layer<T> + Send + Sync>, WorkerGuard), LoggingError>
    where
        T: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        let buf_builder = self.buffer.make_builder();
        let (buf_writer, buf_guard) = self.output.make_writer(buf_builder)?;
        let layer = fmt::layer()
            .with_writer(buf_writer)
            .with_ansi(self.color)
            .log_internal_errors(self.internal_errors)
            .with_target(self.print.target)
            .with_file(self.print.file)
            .with_line_number(self.print.line_number)
            .with_level(self.print.level)
            .with_thread_names(self.print.thread_name)
            .with_thread_ids(self.print.thread_id);
        let boxed_layer = match self.format {
            LoggingFormat::Full => layer.boxed(),
            LoggingFormat::Compact => layer.compact().boxed(),
            LoggingFormat::Pretty => layer.pretty().boxed(),
            LoggingFormat::Json {
                flatten_metadata,
                current_span,
                span_list,
            } => layer
                .json()
                .flatten_event(flatten_metadata)
                .with_current_span(current_span)
                .with_span_list(span_list)
                .boxed(),
        }
        .with_filter(LevelFilter::from(self.level))
        .boxed();
        Ok((boxed_layer, buf_guard))
    }
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase", tag = "format")]
pub enum LoggingFormat {
    ///
    #[default]
    Full,
    ///
    Compact,
    ///
    Pretty,
    ///
    Json {
        ///
        #[serde(default)]
        flatten_metadata: bool,
        ///
        #[serde(default)]
        current_span: bool,
        ///
        #[serde(default)]
        span_list: bool,
    },
}

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LoggingLevel {
    ///
    Off,
    ///
    Error,
    ///
    Warn,
    ///
    Info,
    ///
    #[default]
    Debug,
    ///
    Trace,
}

impl From<LoggingLevel> for LevelFilter {
    fn from(value: LoggingLevel) -> Self {
        match value {
            LoggingLevel::Off => LevelFilter::OFF,
            LoggingLevel::Error => LevelFilter::ERROR,
            LoggingLevel::Warn => LevelFilter::WARN,
            LoggingLevel::Info => LevelFilter::INFO,
            LoggingLevel::Debug => LevelFilter::DEBUG,
            LoggingLevel::Trace => LevelFilter::TRACE,
        }
    }
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct LoggingPrintingConfig {
    ///
    #[serde(default)]
    pub target: bool,
    ///
    #[serde(default)]
    pub file: bool,
    ///
    #[serde(default)]
    pub line_number: bool,
    ///
    #[serde(default = "crate::util::default_true")]
    pub level: bool,
    ///
    #[serde(default)]
    pub thread_name: bool,
    ///
    #[serde(default)]
    pub thread_id: bool,
}

impl Default for LoggingPrintingConfig {
    fn default() -> Self {
        Self {
            target: false,
            file: false,
            line_number: false,
            level: true,
            thread_name: false,
            thread_id: false,
        }
    }
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LoggingBufferConfig {
    ///
    #[serde(default = "LoggingBufferConfig::default_lines")]
    pub lines: usize,
    ///
    #[serde(default = "crate::util::default_true")]
    pub lossy: bool,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_name: Option<String>,
}

impl Default for LoggingBufferConfig {
    fn default() -> Self {
        Self {
            lines: Self::default_lines(),
            lossy: true,
            thread_name: None,
        }
    }
}

impl LoggingBufferConfig {
    /// Default value for [`Self::lines`]
    #[must_use]
    #[inline]
    fn default_lines() -> usize {
        128_000
    }

    ///
    #[must_use]
    pub fn make_builder(&self) -> NonBlockingBuilder {
        let mut builder = NonBlockingBuilder::default()
            .buffered_lines_limit(self.lines)
            .lossy(self.lossy);
        if let Some(thr_name) = &self.thread_name {
            builder = builder.thread_name(thr_name);
        }
        builder
    }

    ///
    pub fn make_writer<W>(&self, ll_writer: W) -> (NonBlocking, WorkerGuard)
    where
        W: io::Write + Send + 'static,
    {
        self.make_builder().finish(ll_writer)
    }
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum LoggingDestination {
    ///
    #[default]
    StdOut,
    ///
    StdErr,
    ///
    File(LoggingFileConfig),
    ///
    Directory(LoggingDirectoryConfig),
}

impl LoggingDestination {
    ///
    pub fn make_writer(
        &self,
        buf_builder: NonBlockingBuilder,
    ) -> Result<(BoxMakeWriter, WorkerGuard), LoggingError> {
        match self {
            Self::StdOut => {
                let (wr, wg) = buf_builder.finish(io::stdout());
                Ok((BoxMakeWriter::new(wr), wg))
            }
            Self::StdErr => {
                let (wr, wg) = buf_builder.finish(io::stderr());
                Ok((BoxMakeWriter::new(wr), wg))
            }
            Self::File(file_cfg) => {
                let file = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&file_cfg.path)?;
                let (wr, wg) = buf_builder.finish(file);
                Ok((BoxMakeWriter::new(wr), wg))
            }
            Self::Directory(dir_cfg) => {
                let appender = RollingFileAppender::builder()
                    .rotation(dir_cfg.rotate.into())
                    .build(&dir_cfg.path)?;
                let (wr, wg) = buf_builder.finish(appender);
                Ok((BoxMakeWriter::new(wr), wg))
            }
        }
    }
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LoggingFileConfig {
    ///
    pub path: String,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LoggingDirectoryConfig {
    ///
    #[serde(default = "LoggingDirectoryConfig::default_path")]
    pub path: String,
    ///
    #[serde(default)]
    pub rotate: LogRotation,
    ///
    #[serde(default)]
    pub prefix: Option<String>,
    ///
    #[serde(default = "LoggingDirectoryConfig::default_suffix")]
    pub suffix: Option<String>,
    ///
    #[serde(default)]
    pub max_files: Option<usize>,
}

impl Default for LoggingDirectoryConfig {
    fn default() -> Self {
        Self {
            path: Self::default_path(),
            rotate: LogRotation::Daily,
            prefix: None,
            suffix: Self::default_suffix(),
            max_files: None,
        }
    }
}

impl LoggingDirectoryConfig {
    /// Default value for [`Self::path`]
    #[must_use]
    #[inline]
    fn default_path() -> String {
        ".".into()
    }

    /// Default value for [`Self::suffix`]
    #[must_use]
    #[inline]
    #[allow(clippy::unnecessary_wraps)]
    fn default_suffix() -> Option<String> {
        Some("log".into())
    }
}

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogRotation {
    ///
    Minutely,
    ///
    Hourly,
    ///
    #[default]
    Daily,
    ///
    Never,
}

impl From<LogRotation> for Rotation {
    fn from(value: LogRotation) -> Self {
        match value {
            LogRotation::Minutely => Rotation::MINUTELY,
            LogRotation::Hourly => Rotation::HOURLY,
            LogRotation::Daily => Rotation::DAILY,
            LogRotation::Never => Rotation::NEVER,
        }
    }
}
