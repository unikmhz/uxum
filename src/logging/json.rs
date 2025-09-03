//! Custom JSON formatter and writer for use in logging.

use std::{borrow::Cow, collections::BTreeMap, fmt, io, marker::PhantomData};

use serde::{ser::SerializeMap, Deserialize, Serialize, Serializer};
use serde_json::{Serializer as JsonSerializer, Value};
use tracing::{Event, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_serde::AsSerde;
use tracing_subscriber::{
    fmt::{
        format::Writer,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields, FormattedFields,
    },
    registry::{LookupSpan, SpanRef},
};

/// Custom names JSON keys.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct JsonKeyNames {
    /// Custom name for `timestamp` key.
    #[serde(default = "JsonKeyNames::default_timestamp")]
    timestamp: Cow<'static, str>,
    /// Custom name for `level` key.
    #[serde(default = "JsonKeyNames::default_level")]
    level: Cow<'static, str>,
    /// Custom name for `fields` key.
    #[serde(default = "JsonKeyNames::default_fields")]
    fields: Cow<'static, str>,
    /// Custom name for `target` key.
    #[serde(default = "JsonKeyNames::default_target")]
    target: Cow<'static, str>,
    /// Custom name for `filename` key.
    #[serde(default = "JsonKeyNames::default_filename")]
    filename: Cow<'static, str>,
    /// Custom name for `line_number` key.
    #[serde(default = "JsonKeyNames::default_line_number")]
    line_number: Cow<'static, str>,
    /// Custom name for `span` key.
    #[serde(default = "JsonKeyNames::default_span")]
    span: Cow<'static, str>,
    /// Custom name for `threadName` key.
    #[serde(default = "JsonKeyNames::default_thread_name")]
    thread_name: Cow<'static, str>,
    /// Custom name for `threadId` key.
    #[serde(default = "JsonKeyNames::default_thread_id")]
    thread_id: Cow<'static, str>,
}

impl Default for JsonKeyNames {
    fn default() -> Self {
        Self {
            timestamp: Self::default_timestamp(),
            level: Self::default_level(),
            fields: Self::default_fields(),
            target: Self::default_target(),
            filename: Self::default_filename(),
            line_number: Self::default_line_number(),
            span: Self::default_span(),
            thread_name: Self::default_thread_name(),
            thread_id: Self::default_thread_id(),
        }
    }
}

impl JsonKeyNames {
    /// Default value for [`Self::timestamp`].
    #[must_use]
    #[inline]
    fn default_timestamp() -> Cow<'static, str> {
        Cow::Borrowed("timestamp")
    }

    /// Default value for [`Self::level`].
    #[must_use]
    #[inline]
    fn default_level() -> Cow<'static, str> {
        Cow::Borrowed("level")
    }

    /// Default value for [`Self::fields`].
    #[must_use]
    #[inline]
    fn default_fields() -> Cow<'static, str> {
        Cow::Borrowed("fields")
    }

    /// Default value for [`Self::target`].
    #[must_use]
    #[inline]
    fn default_target() -> Cow<'static, str> {
        Cow::Borrowed("target")
    }

    /// Default value for [`Self::filename`].
    #[must_use]
    #[inline]
    fn default_filename() -> Cow<'static, str> {
        Cow::Borrowed("filename")
    }

    /// Default value for [`Self::line_number`].
    #[must_use]
    #[inline]
    fn default_line_number() -> Cow<'static, str> {
        Cow::Borrowed("line_number")
    }

    /// Default value for [`Self::span`].
    #[must_use]
    #[inline]
    fn default_span() -> Cow<'static, str> {
        Cow::Borrowed("span")
    }

    /// Default value for [`Self::thread_name`].
    #[must_use]
    #[inline]
    fn default_thread_name() -> Cow<'static, str> {
        Cow::Borrowed("threadName")
    }

    /// Default value for [`Self::thread_id`].
    #[must_use]
    #[inline]
    fn default_thread_id() -> Cow<'static, str> {
        Cow::Borrowed("threadId")
    }
}

/// Extensible JSON format.
///
/// Similar to [`tracing_subscriber::fmt::format::Json`], but with extra features.
#[derive(Clone, Debug)]
pub(crate) struct ExtensibleJsonFormat<T = SystemTime> {
    /// Time formatter.
    timer: T,
    /// Add timestamps to output.
    display_timestamp: bool,
    /// Add span target to output.
    display_target: bool,
    /// Add event level to output.
    display_level: bool,
    /// Add thread ID to output.
    display_thread_id: bool,
    /// Add thread name to output.
    display_thread_name: bool,
    /// Add filename of source code origin to output.
    display_filename: bool,
    /// Add file line of source code origin to output.
    display_line_number: bool,
    /// Flatten event metadata.
    flatten_event: bool,
    /// Add current span info to output.
    display_current_span: bool,
    /// Static fields to add to output.
    static_fields: BTreeMap<String, Value>,
    /// Custom names to use for JSON object keys.
    key_names: JsonKeyNames,
}

impl Default for ExtensibleJsonFormat {
    fn default() -> Self {
        Self {
            timer: SystemTime,
            display_timestamp: true,
            display_target: true,
            display_level: true,
            display_thread_id: false,
            display_thread_name: false,
            display_filename: false,
            display_line_number: false,
            flatten_event: false,
            display_current_span: true,
            static_fields: BTreeMap::new(),
            key_names: JsonKeyNames::default(),
        }
    }
}

impl<S, N, T> FormatEvent<S, N> for ExtensibleJsonFormat<T>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut timestamp = String::new();
        self.timer.format_time(&mut Writer::new(&mut timestamp))?;

        let meta = event.normalized_metadata();
        let meta = meta.as_ref().unwrap_or_else(|| event.metadata());

        let mut visit = || {
            let mut ser = JsonSerializer::new(JsonWriter::new(&mut writer));
            let mut ser = ser.serialize_map(None)?;

            if self.display_timestamp {
                ser.serialize_entry(self.key_names.timestamp.as_ref(), &timestamp)?;
            }

            if self.display_level {
                ser.serialize_entry(self.key_names.level.as_ref(), &meta.level().as_serde())?;
            }

            let format_field_marker: PhantomData<N> = PhantomData;

            let current_span = if self.display_current_span {
                event
                    .parent()
                    .and_then(|id| ctx.span(id))
                    .or_else(|| ctx.lookup_current())
            } else {
                None
            };

            if self.flatten_event {
                let mut visitor = tracing_serde::SerdeMapVisitor::new(ser);
                event.record(&mut visitor);
                ser = visitor.take_serializer()?;
            } else {
                use tracing_serde::fields::AsMap;
                ser.serialize_entry(self.key_names.fields.as_ref(), &event.field_map())?;
            }

            if self.display_target {
                ser.serialize_entry(self.key_names.target.as_ref(), meta.target())?;
            }

            if self.display_filename {
                if let Some(filename) = meta.file() {
                    ser.serialize_entry(self.key_names.filename.as_ref(), filename)?;
                }
            }

            if self.display_line_number {
                if let Some(line_number) = meta.line() {
                    ser.serialize_entry(self.key_names.line_number.as_ref(), &line_number)?;
                }
            }

            if self.display_current_span {
                if let Some(ref span) = current_span {
                    ser.serialize_entry(
                        self.key_names.span.as_ref(),
                        &SerializableSpan(span, format_field_marker),
                    )
                    .unwrap_or(());
                }
            }

            if self.display_thread_name {
                let current_thread = std::thread::current();
                match current_thread.name() {
                    Some(name) => ser.serialize_entry(self.key_names.thread_name.as_ref(), name)?,
                    // fall-back to thread id when name is absent and ids are not enabled
                    None if !self.display_thread_id => {
                        ser.serialize_entry(
                            self.key_names.thread_name.as_ref(),
                            &format!("{:?}", current_thread.id()),
                        )?;
                    }
                    _ => {}
                }
            }

            if self.display_thread_id {
                ser.serialize_entry(
                    self.key_names.thread_id.as_ref(),
                    &format!("{:?}", std::thread::current().id()),
                )?;
            }

            for (key, val) in &self.static_fields {
                ser.serialize_entry(key, val)?;
            }

            ser.end()
        };

        visit().map_err(|_| fmt::Error)?;
        writeln!(writer)
    }
}

impl ExtensibleJsonFormat {
    /// Create new JSON formatter.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl<T> ExtensibleJsonFormat<T> {
    /// Use the given [`timer`] for log message timestamps.
    ///
    /// See [`time` module] for the provided timer implementations.
    ///
    /// Note that using the `"time"` feature flag enables the
    /// additional time formatters [`UtcTime`] and [`LocalTime`], which use the
    /// [`time` crate] to provide more sophisticated timestamp formatting
    /// options.
    ///
    /// [`timer`]: tracing_subscriber::fmt::time::FormatTime
    /// [`time` module]: mod@tracing_subscriber::fmt::time
    /// [`UtcTime`]: tracing_subscriber::fmt::time::UtcTime
    /// [`LocalTime`]: tracing_subscriber::fmt::time::LocalTime
    /// [`time` crate]: https://docs.rs/time/0.3
    #[allow(dead_code)]
    pub(crate) fn with_timer<T2>(self, timer: T2) -> ExtensibleJsonFormat<T2> {
        ExtensibleJsonFormat {
            timer,
            display_timestamp: self.display_timestamp,
            display_target: self.display_target,
            display_level: self.display_level,
            display_thread_id: self.display_thread_id,
            display_thread_name: self.display_thread_name,
            display_filename: self.display_filename,
            display_line_number: self.display_line_number,
            flatten_event: self.flatten_event,
            display_current_span: self.display_current_span,
            static_fields: self.static_fields,
            key_names: self.key_names,
        }
    }

    /// Do not emit timestamps with log messages.
    #[allow(dead_code)]
    pub(crate) fn without_time(self) -> ExtensibleJsonFormat<()> {
        ExtensibleJsonFormat {
            timer: (),
            display_timestamp: false,
            display_target: self.display_target,
            display_level: self.display_level,
            display_thread_id: self.display_thread_id,
            display_thread_name: self.display_thread_name,
            display_filename: self.display_filename,
            display_line_number: self.display_line_number,
            flatten_event: self.flatten_event,
            display_current_span: self.display_current_span,
            static_fields: self.static_fields,
            key_names: self.key_names,
        }
    }

    /// Sets whether or not an event's target is displayed.
    pub(crate) fn with_target(self, display_target: bool) -> Self {
        Self {
            display_target,
            ..self
        }
    }

    /// Sets whether or not an event's level is displayed.
    pub(crate) fn with_level(self, display_level: bool) -> Self {
        Self {
            display_level,
            ..self
        }
    }

    /// Sets whether or not the [thread ID] of the current thread is displayed
    /// when formatting events.
    ///
    /// [thread ID]: std::thread::ThreadId
    pub(crate) fn with_thread_ids(self, display_thread_id: bool) -> Self {
        Self {
            display_thread_id,
            ..self
        }
    }

    /// Sets whether or not the [name] of the current thread is displayed
    /// when formatting events.
    ///
    /// [name]: std::thread#naming-threads
    pub(crate) fn with_thread_names(self, display_thread_name: bool) -> Self {
        Self {
            display_thread_name,
            ..self
        }
    }

    /// Sets whether or not an event's [source code file path][file] is
    /// displayed.
    ///
    /// [file]: tracing::Metadata::file
    pub(crate) fn with_file(self, display_filename: bool) -> Self {
        Self {
            display_filename,
            ..self
        }
    }

    /// Sets whether or not an event's [source code line number][line] is
    /// displayed.
    ///
    /// [line]: tracing::Metadata::line
    pub(crate) fn with_line_number(self, display_line_number: bool) -> Self {
        Self {
            display_line_number,
            ..self
        }
    }

    /// Use the full JSON format with the event's event fields flattened.
    ///
    /// # Example Output
    ///
    /// ```ignore,json
    /// {"timestamp":"Feb 20 11:28:15.096","level":"INFO","target":"mycrate", "message":"some message", "key": "value"}
    /// ```
    pub(crate) fn flatten_event(self, flatten_event: bool) -> Self {
        Self {
            flatten_event,
            ..self
        }
    }

    /// Sets whether or not the formatter will include the current span in
    /// formatted events.
    pub(crate) fn with_current_span(self, display_current_span: bool) -> Self {
        Self {
            display_current_span,
            ..self
        }
    }

    /// Add static fields to generated JSON objects.
    pub(crate) fn with_static_fields(self, mut static_fields: BTreeMap<String, Value>, parse_env: bool) -> Self {
        if parse_env {
            for (_, val) in static_fields.iter_mut() {
                if let Some(value) = val.as_str() {
                    if value.starts_with('$') {
                        let value = value.strip_prefix('$').unwrap_or_default();
                        if let Ok(value) = std::env::var(value) {
                            *val = value.into();
                        }
                    }
                }
            }
        }

        Self {
            static_fields,
            ..self
        }
    }

    /// Set custom JSON field names.
    pub(crate) fn with_key_names(self, key_names: JsonKeyNames) -> Self {
        Self { key_names, ..self }
    }
}

struct SerializableSpan<'a, 'b, Span, N>(&'b SpanRef<'a, Span>, PhantomData<N>)
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static;

impl<Span, N> Serialize for SerializableSpan<'_, '_, Span, N>
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: Serializer,
    {
        let mut serializer = serializer.serialize_map(None)?;

        let ext = self.0.extensions();
        let data = ext
            .get::<FormattedFields<N>>()
            .expect("Unable to find FormattedFields in extensions; this is a bug");

        // TODO: let's _not_ do this, but this resolves
        // https://github.com/tokio-rs/tracing/issues/391.
        // We should probably rework this to use a `serde_json::Value` or something
        // similar in a JSON-specific layer, but I'd (david)
        // rather have a uglier fix now rather than shipping broken JSON.
        match serde_json::from_str::<Value>(data) {
            Ok(Value::Object(fields)) => {
                for field in fields {
                    serializer.serialize_entry(&field.0, &field.1)?;
                }
            }
            // We have fields for this span which are valid JSON but not an object.
            // This is probably a bug, so panic if we're in debug mode.
            Ok(_) if cfg!(debug_assertions) => panic!(
                "span '{}' had malformed fields! this is a bug.\n  error: invalid JSON object\n  fields: {:?}",
                self.0.metadata().name(),
                data
            ),
            // If we *aren't* in debug mode, it's probably best not to
            // crash the program, let's log the field found but also an
            // message saying it's type is invalid.
            Ok(value) => {
                serializer.serialize_entry("field", &value)?;
                serializer.serialize_entry("field_error", "field was no a valid object")?
            }
            // We have previously recorded fields for this span
            // should be valid JSON. However, they appear to *not*
            // be valid JSON. This is almost certainly a bug, so
            // panic if we're in debug mode.
            Err(e) if cfg!(debug_assertions) => panic!(
                "span '{}' had malformed fields! this is a bug.\n  error: {}\n  fields: {:?}",
                self.0.metadata().name(),
                e,
                data
            ),
            // If we *aren't* in debug mode, it's probably best not
            // crash the program, but let's at least make sure it's clear
            // that the fields are not supposed to be missing.
            Err(e) => serializer.serialize_entry("field_error", &format!("{e}"))?,
        };
        serializer.serialize_entry("name", self.0.metadata().name())?;
        serializer.end()
    }
}

pub(crate) struct JsonWriter<'a> {
    fmt_write: &'a mut dyn fmt::Write,
}

impl<'a> JsonWriter<'a> {
    pub(crate) fn new(fmt_write: &'a mut dyn fmt::Write) -> Self {
        Self { fmt_write }
    }
}

impl io::Write for JsonWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s =
            std::str::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        self.fmt_write.write_str(s).map_err(io::Error::other)?;

        Ok(s.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
