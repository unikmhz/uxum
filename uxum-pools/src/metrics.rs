use std::{
    borrow::Cow,
    sync::{Arc, LazyLock},
};

use opentelemetry::{
    global,
    metrics::{Gauge, Histogram},
    Key, KeyValue, StringValue, Value,
};

/// Central metrics singleton for instrumented pool metrics.
pub(crate) static POOL_METRICS: LazyLock<Arc<Metrics>> = LazyLock::new(|| Arc::new(Metrics::new()));

const KEY_POOL_NAME: Key = Key::from_static_str("db.client.connection.pool.name");
const KEY_STATE: Key = Key::from_static_str("db.client.connection.state");

/// Storage for pool metrics.
pub(crate) struct Metrics {
    /// The number of connections that are currently in state described by the state attribute.
    pub conn_count: Gauge<u64>,
    /// The time it took to obtain an open connection from the pool.
    pub wait_time: Histogram<f64>,
    /// The time between borrowing a connection and returning it to the pool.
    pub use_time: Histogram<f64>,
    /// Duration of database client operations.
    pub op_duration: Histogram<f64>,
    /// The maximum number of idle open connections allowed.
    pub idle_max: Gauge<u64>,
    /// The minimum number of idle open connections allowed.
    pub idle_min: Gauge<u64>,
    /// The maximum number of open connections allowed.
    pub conn_max: Gauge<u64>,
}

impl Metrics {
    /// Create new storage for pool metrics.
    ///
    /// You probably don't need this, as all pools use a central metrics singleton for storage.
    pub(crate) fn new() -> Self {
        let meter = global::meter("uxum-pools");
        // db.client.connection.pool.name (string)
        // db.client.connection.state (idle / used)
        // TODO: unused
        let conn_count = meter
            .u64_gauge("db.client.connection.count")
            .with_description("The number of connections that are currently in state described by the state attribute.")
            .init();
        // db.client.connection.pool.name (string)
        let wait_time = meter
            .f64_histogram("db.client.connection.wait_time")
            .with_unit("s")
            .with_description("The time it took to obtain an open connection from the pool.")
            .init();
        // db.client.connection.pool.name (string)
        let use_time = meter
            .f64_histogram("db.client.connection.use_time")
            .with_unit("s")
            .with_description(
                "The time between borrowing a connection and returning it to the pool.",
            )
            .init();
        // db.system.name (string, DB type)
        // db.collection.name (string, table)
        // db.namespace (string, database/schema, fully qualified)
        // db.operation.name (string, command)
        // db.response.status_code (string, error or exception code)
        // error.type (string, class of error)
        // server.port (int, local service)
        // db.query.summary (string, abbreviated query)
        // network.peer.address (string, raw remote addr of DB node)
        // network.peer.port (int, remote port)
        // server.address (string, name of DB host)
        // db.query.text (string, query without binds, dubious)
        let op_duration = meter
            .f64_histogram("db.client.operation.duration")
            .with_unit("s")
            .with_description("Duration of database client operations.")
            .init();
        // db.client.connection.pool.name (string)
        let idle_max = meter
            .u64_gauge("db.client.connection.idle.max")
            .with_description("The maximum number of idle open connections allowed.")
            .init();
        // db.client.connection.pool.name (string)
        let idle_min = meter
            .u64_gauge("db.client.connection.idle.min")
            .with_description("The minimum number of idle open connections allowed.")
            .init();
        // db.client.connection.pool.name (string)
        let conn_max = meter
            .u64_gauge("db.client.connection.max")
            .with_description("The maximum number of open connections allowed.")
            .init();
        Metrics {
            conn_count,
            wait_time,
            use_time,
            op_duration,
            idle_max,
            idle_min,
            conn_max,
        }
    }

    pub(crate) fn record_state(&self, label: &[KeyValue], state: PoolState) {
        if let Some(max_size) = state.max_size {
            self.conn_max.record(max_size as u64, label);
        }
        if let Some(size) = state.size {
            let total_label = status_kv(label[0].clone(), "total");
            self.conn_count.record(size as u64, &total_label);
        }
        if let Some(idle) = state.idle {
            let idle_label = status_kv(label[0].clone(), "idle");
            self.conn_count.record(idle as u64, &idle_label);
        }
        if let Some(in_use) = state.in_use {
            let used_label = status_kv(label[0].clone(), "used");
            self.conn_count.record(in_use as u64, &used_label);
        }
        if let Some(min_idle) = state.min_idle {
            self.idle_min.record(min_idle as u64, label);
        }
        if let Some(max_idle) = state.max_idle {
            self.idle_max.record(max_idle as u64, label);
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics::new()
    }
}

pub(crate) fn pool_kv(name: Option<Cow<'static, str>>) -> [KeyValue; 1] {
    match name {
        Some(n) => [KeyValue::new(KEY_POOL_NAME, n)],
        None => [KeyValue::new(KEY_POOL_NAME, "default")],
    }
}

pub(crate) fn status_kv(name: KeyValue, status: &'static str) -> [KeyValue; 2] {
    [
        name,
        KeyValue::new(KEY_STATE, Value::String(StringValue::from(status))),
    ]
}

/// State of a pool object.
///
/// If pool type doesn't support some metrics, these metrics must be left as `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PoolState {
    /// Maximum total (`idle` + `in_use`) number of resources in the pool.
    pub max_size: Option<usize>,
    /// Current total (`idle` + `in_use`) number of resources in the pool.
    pub size: Option<usize>,
    /// Current number of idle (not acquired) resources.
    pub idle: Option<usize>,
    /// Current number of in-use (acquired) resources.
    pub in_use: Option<usize>,
    /// Minimum number of idle resources to keep in the pool.
    pub min_idle: Option<usize>,
    /// Maximum number of idle resources to keep in the pool.
    pub max_idle: Option<usize>,
}
