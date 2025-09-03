//! Logging configuration via [`tracing`] crate.

pub(crate) mod json;
pub(crate) mod span;

use std::{collections::BTreeMap, fs, io};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_appender::{
    non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    fmt::{self, writer::BoxMakeWriter},
    layer::{Layer, Layered, SubscriberExt},
    registry::Registry,
};

use crate::logging::json::{ExtensibleJsonFormat, JsonKeyNames};

type LoggingRegistry = Layered<Vec<Box<dyn Layer<Registry> + Send + Sync>>, Registry>;

/// Error type used in logging configuration.
#[derive(Debug, Error)]
pub enum LoggingError {
    /// Log destination I/O error.
    #[error("Log destination I/O error: {0}")]
    Io(#[from] io::Error),
    /// Error while initializing log directory writer.
    #[error("Error while initializing log directory writer: {0}")]
    Directory(#[from] tracing_appender::rolling::InitError),
}

/// Logging configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LoggingConfig {
    /// List of subscribers defined in configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscribers: Vec<LoggingSubscriberConfig>,
}

impl LoggingConfig {
    /// Create registry subscriber from configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any of the subscribers cannot be initialized.
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

/// Individual logging subscriber configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LoggingSubscriberConfig {
    /// Overall format for logging output.
    #[serde(default, flatten)]
    pub format: LoggingFormat,
    /// Minimum severity level to include in output.
    #[serde(default)]
    pub level: LoggingLevel,
    /// Custom minimum severity levels for span targets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<String, LoggingLevel>,
    /// Use ANSI escape sequences for output colors and formatting.
    #[serde(default)]
    pub color: bool,
    /// Include errors of logging subsystem in output.
    #[serde(default = "crate::util::default_true")]
    pub internal_errors: bool,
    /// Additional span information to include in output.
    #[serde(default)]
    pub print: LoggingPrintingConfig,
    /// Write buffer configuration for a non-blocking writer.
    #[serde(default)]
    pub buffer: LoggingBufferConfig,
    /// Log destination configuration.
    #[serde(default)]
    pub output: LoggingDestination,
}

impl Default for LoggingSubscriberConfig {
    fn default() -> Self {
        Self {
            format: LoggingFormat::default(),
            level: LoggingLevel::default(),
            targets: BTreeMap::new(),
            color: false,
            internal_errors: true,
            print: LoggingPrintingConfig::default(),
            buffer: LoggingBufferConfig::default(),
            output: LoggingDestination::default(),
        }
    }
}

impl LoggingSubscriberConfig {
    /// Logging subscriber template for use in development.
    #[must_use]
    pub fn default_for_dev() -> Self {
        Self {
            format: LoggingFormat::Pretty,
            level: LoggingLevel::Trace,
            targets: BTreeMap::new(),
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

    /// Make [`tracing_subscriber::Layer`] from subscriber configuration.
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
                ref static_fields,
                ref key_names,
                parse_env_in_static,
            } => {
                let json_fmt = ExtensibleJsonFormat::new()
                    .with_target(self.print.target)
                    .with_file(self.print.file)
                    .with_line_number(self.print.line_number)
                    .with_level(self.print.level)
                    .with_thread_names(self.print.thread_name)
                    .with_thread_ids(self.print.thread_id)
                    .flatten_event(flatten_metadata)
                    .with_current_span(current_span)
                    .with_static_fields(static_fields.clone(), parse_env_in_static)
                    .with_key_names(*key_names.clone());
                layer.json().event_format(json_fmt).boxed()
            }
        };
        let boxed_layer = if self.targets.is_empty() {
            boxed_layer
                .with_filter(LevelFilter::from(self.level))
                .boxed()
        } else {
            boxed_layer
                .with_filter(
                    Targets::new()
                        .with_targets(self.targets.clone())
                        .with_default(LevelFilter::from(self.level)),
                )
                .boxed()
        };
        Ok((boxed_layer, buf_guard))
    }
}

/// Format for logging output.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase", tag = "format")]
pub enum LoggingFormat {
    /// Format which prints span context before log message.
    ///
    /// See [`tracing_subscriber::fmt::format::Full`].
    #[default]
    Full,
    /// More compact format, span names are hidden.
    ///
    /// See [`tracing_subscriber::fmt::format::Compact`].
    Compact,
    /// Excessively verbose and pretty multiline format.
    ///
    /// Might be useful when developing and testing code.
    /// See [`tracing_subscriber::fmt::format::Pretty`].
    Pretty,
    /// Formats logs as newline-delimited JSON objects.
    ///
    /// See [`tracing_subscriber::fmt::format::Json`].
    Json {
        /// Flatten event metadata fields into object.
        ///
        /// See [`tracing_subscriber::fmt::format::Json::flatten_event`].
        #[serde(default)]
        flatten_metadata: bool,
        /// Add current span name to object.
        ///
        /// See [`tracing_subscriber::fmt::format::Json::with_current_span`].
        #[serde(default)]
        current_span: bool,
        /// Static fields to add to object.
        #[serde(default)]
        static_fields: BTreeMap<String, serde_json::Value>,
        /// Custom names for JSON keys.
        #[serde(default)]
        key_names: Box<JsonKeyNames>,
        /// Parse environment variables in static fields
        #[serde(default)]
        parse_env_in_static: bool,
    },
}

/// Minumum event severity for log output.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LoggingLevel {
    /// Disables logging altogether.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::OFF`].
    #[serde(alias = "off", alias = "disabled", alias = "DISABLED")]
    Off,
    /// Write "error" level only.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::ERROR`].
    #[serde(alias = "error", alias = "err", alias = "ERR")]
    Error,
    /// Write "warn" and more severe levels.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::WARN`].
    #[serde(alias = "warn", alias = "warning", alias = "WARNING")]
    Warn,
    /// Write "info" and more severe levels.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::INFO`].
    #[serde(alias = "info")]
    Info,
    /// Write "debug" and more severe levels.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::DEBUG`].
    #[serde(alias = "debug")]
    #[default]
    Debug,
    /// Write everything.
    ///
    /// See [`tracing_subscriber::filter::LevelFilter::TRACE`].
    #[serde(alias = "trace")]
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

/// Additional information to include in output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)]
pub struct LoggingPrintingConfig {
    /// Print event target.
    #[serde(default)]
    pub target: bool,
    /// Print source file path.
    #[serde(default)]
    pub file: bool,
    /// Print source line number.
    #[serde(default)]
    pub line_number: bool,
    /// Print severity level.
    #[serde(default = "crate::util::default_true")]
    pub level: bool,
    /// Print thread name.
    #[serde(default)]
    pub thread_name: bool,
    /// Print thread ID.
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

/// Configuration for a non-blocking writer.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LoggingBufferConfig {
    /// Maximum buffered lines to store.
    ///
    /// After reaching this number of lines, new events will either be dropped or block, depending
    /// on [`Self::lossy`] parameter.
    ///
    /// See [`tracing_appender::non_blocking::NonBlockingBuilder::buffered_lines_limit`].
    #[serde(default = "LoggingBufferConfig::default_lines")]
    pub lines: usize,
    /// What to do with log lines that cannot be added to the buffer.
    ///
    /// Either drop them if `true`, or block execution until there is sufficient space in the buffer
    /// if `false`.
    ///
    /// See [`tracing_appender::non_blocking::NonBlockingBuilder::lossy`].
    #[serde(default = "crate::util::default_true")]
    pub lossy: bool,
    /// Override the thread name of log appender.
    ///
    /// See [`tracing_appender::non_blocking::NonBlockingBuilder::thread_name`].
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
    /// Default value for [`Self::lines`].
    #[must_use]
    #[inline]
    fn default_lines() -> usize {
        128_000
    }

    /// Construct a builder for non-blocking writer.
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

    /// Construct a non-blocking writer.
    pub fn make_writer<W>(&self, ll_writer: W) -> (NonBlocking, WorkerGuard)
    where
        W: io::Write + Send + 'static,
    {
        self.make_builder().finish(ll_writer)
    }
}

/// Logging output destination configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum LoggingDestination {
    /// Output to standard output (stdout, fd 1).
    #[default]
    #[serde(alias = "stdout", alias = "out")]
    StdOut,
    /// Output to standard error (stderr, fd 2).
    #[serde(alias = "stderr", alias = "err")]
    StdErr,
    /// Output to file.
    File(LoggingFileConfig),
    /// Output to files in a directory with optional rotation.
    #[serde(alias = "dir")]
    Directory(LoggingDirectoryConfig),
}

impl LoggingDestination {
    /// Make [`tracing_subscriber::fmt::writer::BoxMakeWriter`] from configuration.
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
                let mut builder = RollingFileAppender::builder().rotation(dir_cfg.rotate.into());
                if let Some(prefix) = &dir_cfg.prefix {
                    builder = builder.filename_prefix(prefix);
                }
                if let Some(suffix) = &dir_cfg.suffix {
                    builder = builder.filename_suffix(suffix);
                }
                if let Some(max_files) = dir_cfg.max_files {
                    builder = builder.max_log_files(max_files);
                }
                let appender = builder.build(&dir_cfg.path)?;
                let (wr, wg) = buf_builder.finish(appender);
                Ok((BoxMakeWriter::new(wr), wg))
            }
        }
    }
}

/// Configuration of file output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LoggingFileConfig {
    /// Path to file.
    pub path: String,
}

/// Configuration of directory output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LoggingDirectoryConfig {
    /// Path to directory.
    #[serde(default = "LoggingDirectoryConfig::default_path")]
    pub path: String,
    /// Log rotation configuration.
    #[serde(default)]
    pub rotate: LogRotation,
    /// Template prefix for file names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Template suffix for file names.
    #[serde(default = "LoggingDirectoryConfig::default_suffix")]
    pub suffix: Option<String>,
    /// Maximum amount of files to keep in directory.
    #[serde(default)]
    pub max_files: Option<usize>,
}

impl Default for LoggingDirectoryConfig {
    fn default() -> Self {
        Self {
            path: Self::default_path(),
            rotate: LogRotation::default(),
            prefix: None,
            suffix: Self::default_suffix(),
            max_files: None,
        }
    }
}

impl LoggingDirectoryConfig {
    /// Default value for [`Self::path`].
    #[must_use]
    #[inline]
    fn default_path() -> String {
        ".".into()
    }

    /// Default value for [`Self::suffix`].
    #[must_use]
    #[inline]
    #[allow(clippy::unnecessary_wraps)]
    fn default_suffix() -> Option<String> {
        Some("log".into())
    }
}

/// Log rotation configuration.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "UPPERCASE")]
pub enum LogRotation {
    /// Rotate logs once every minute.
    Minutely,
    /// Rotate logs once every hour.
    Hourly,
    /// Rotate logs once every day.
    #[default]
    Daily,
    /// Never rotate logs.
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
