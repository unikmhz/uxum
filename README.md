# UXUM

[![crates.io](https://img.shields.io/crates/v/uxum.svg)](https://crates.io/crates/uxum)
[![build status](https://img.shields.io/github/actions/workflow/status/unikmhz/uxum/ci.yml?branch=main&logo=github)](https://github.com/unikmhz/uxum/actions)
[![license](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue)](#license)
[![documentation](https://docs.rs/uxum/badge.svg)](https://docs.rs/uxum/)

An opinionated backend service framework based on axum.

## Project goals

 * Minimum boilerplate code.
 * Minimal performance impact from features not in use.
 * Metrics, tracing, OpenAPI and common service support features available out of the box.
 * Ready to be deployed on a local server, VM or container, or in the cloud.

## Project non-goals

 * Performance and feature parity with bare axum.
   Straight-up axum without all bells and whistles provided by this framework will always be a bit faster
   and more flexible.
 * Database access layers and connection pools.
   This is out of scope for this project.

## Supported crate features

 * `grpc`: support nesting Tonic GRPC services inside Axum server instance.
 * `hash_argon2`: support PHC user password hashes using [Argon2](https://docs.rs/argon2) algorithm.
 * `hash_pbkdf2`: support PHC user password hashes using [PBKDF2](https://docs.rs/pbkdf2) and HMAC-SHA256/512 algorithm.
 * `hash_scrypt`: support PHC user password hashes using [SCrypt](https://docs.rs/scrypt) algorithm.
 * `hash_all`: alias for `hash_argon2` + `hash_pbkdf2` + `hash_scrypt`.
 * `jwt`: support athentication via HTTP Bearer using [JWT](https://datatracker.ietf.org/doc/html/rfc7519).
 * `kafka`: support writing logs to a Kafka topic.
 * `spiffe`: support mTLS transport with SPIFFE authentication and authorization.
 * `systemd`: enable systemd integration for service notifications and watchdog support (Linux only).
 * `full`: kitchen sink mode, enable every feature.
 * `http_log_gzip`: support GZIP-compression of the logs request body (relevant only for logs http-delivery).
 * `http_log_zstd`: support ZSTD-compression of the logs request body (relevant only for logs http-delivery).

## Quick start guide

Quick guide to bootstrapping you own service based on `uxum`.

### Add framework to your dependencies

Put this in your project's `Cargo.toml`:

```toml
[dependencies]
anyhow = "1.0"
clap = { version = "4.5", features = ["derive", "env"] }
uxum = { version = "0.10", features = ["systemd"] }
```

### Create a minimal working service

Following example is artificially split into parts for ease of reading.

```rust,no_run

//
// 1. Create a CLI argument parser.
//    This is not strictly necessary, but is nice to have.
//

use clap::Parser;

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to configuration file.
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "config.yaml",
        env = "SOME_SERVICE_CONFIG_FILE"

    )]
    config_file: String,
}

//
// 2. Write a program entry point.
//    Here we do not have async runtime yet, so we create and run it.
//

use uxum::prelude::*;

/// Application entry point.
fn main() -> Result<(), anyhow::Error> {
    // Parse CLI arguments.
    let args = Args::parse();
    eprintln!("CLI args: {args:?}");
    // Merge, load and deserialize configuration.
    // [`ServiceConfig`] is a handy type that contains all uxum-related configuration entries,
    // and can also contain your application configuration.
    //
    // Your configuration-holding type can be passed as a parameter to [`ServiceConfig`].
    // Default parameter value is `()`, which means "no application configuration exists".
    let mut config = ServiceConfig::builder()
        .with_file(&args.config_file) // Load parameters from configuration file.
        .with_env("SOME_SERVICE") // Variable name prefix to override specific parameters via env.
        .build()?;
    // Add some hard-coded values to [`AppConfig`]. Application name and version are handy to
    // get automatically from Cargo.
    let app_cfg = config
        .app
        .with_app_name(env!("CARGO_PKG_NAME"))
        .with_app_version(env!("CARGO_PKG_VERSION"));
    // Build and start Tokio runtime.
    app_cfg.runtime.build()?.block_on(run(args, config))
}

//
// 3. Write an async entry point.
//    Here we apply all settings, start the service and begin serving requests.
//

use std::{net::SocketAddr, time::Duration};

/// Tokio runtime entry point.
async fn run(args: Args, mut config: ServiceConfig) -> Result<(), anyhow::Error> {
    // Initialize uxum handle, including logging, monitoring and tracing.
    //
    // Logging will start working right after this call, and until the returned
    // guard is dropped.
    let mut handle = config.app.handle().await?;
    // Create app builder from app config.
    let mut app_builder = AppBuilder::from_config(&config.app)?;
    // Some hard-coded parameters for built-in API documentation.
    app_builder.configure_api_doc(|api_doc| {
        api_doc
            .with_app_title("Some Service")
            .with_description("Service description.")
            .with_contact_name("Your organization name")
            .with_contact_url("http://example.com")
            .with_contact_email("example@example.com")
    });
    // Build main application router.
    // This is the meat of the operation. All handlers are registered, processed,
    // dressed into [`tower`] layers, and finally merged into application router.
    // Some support stuff is initialized as well, such as probes, API documentation,
    // gRPC services and fallback/error handlers.
    let app = app_builder.build()?;
    // Convert axum router into tower service.
    let svc = app.into_make_service_with_connect_info::<SocketAddr>();
    // Start the service.
    // This internally executes `axum-server` start routines. Note that this might
    // start more than one server, if for example you have specified both TLS and
    // non-TLS listener configuration.
    handle
        .run(config.servers, svc, Some(Duration::from_secs(5)))
        .await
        .map_err(Into::into)
}

//
// 4. Create your endpoint.
//    Or 100 endpoints.
//

/// Example handler.
///
/// See following sections for a detailed primer on handler parameters.
/// But at the end of the day -- this is just a normal axum request handler, and as such,
/// it can access all the parameters/extractors a normal handler can.
#[handler(path = "/some/method")]
async fn some_method() -> String {
    "Hello, world!".into()
}

```

This is only a template. Feel free to customize the calls, split the code, add your own states etc.

More extensive examples can be viewed from [repository](https://github.com/unikmhz/uxum/tree/main/examples).

## Features

### Simplified automatic routing

### OpenAPI specification and API browser

### Logging

### Metrics

### Tracing

### Application behavior

You can implement a custom behaviour object for your application, to customize some of this framework's
features. Currently these customizations are available:

* Provide global [`tower`] layers to wrap all your handlers with.
* Customize liveness and readiness probes.
* Customize error handling for method calls.

Consult documentation for [`crate::AppBehavior`] for further information.

Brief example:
```rust,no_run
# use std::convert::Infallible;
# use uxum::prelude::*;
# use axum::{BoxError, body::Body, http::{Response, StatusCode}, response::IntoResponse, routing::MethodRouter};
#[derive(Clone)]
pub struct CustomBehavior;

impl AppBehavior for CustomBehavior {
    fn error_layer(rtr: MethodRouter<(), BoxError>) -> MethodRouter<(), Infallible> {
        rtr.handle_error(some_custom_async_func)
    }
}

async fn some_custom_async_func(_err: BoxError) -> Response<Body> {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
```

### HTTP clients

You can create any number of integrated HTTP clients from [`AppBuilder`] when initializing your app,
like this:

```rust,ignore
let client = app_builder.http_client_or_default("client_name").await?;
```

`client_name` here is used as:

1. key to look up client configuration.
2. as a label for metrics and tracing.

These clients are [`reqwest_middleware`] client wrappers over [`reqwest`] clients. You can pass them
to you application's state, or use them in any other way you like.

These clients will benefit from automatic integration with application metrics, distributed tracing
and advanced middleware features provided by this framework.

### gRPC support

## Handler parameters

This section describes all available parameters for a `#[handler]` procedural macro.

Multiple parameters can be inserted into `#[handler]` macro, like this:

```rust,ignore
#[handler(parameter1, parameter2, parameter3 = "value", parameter4 = ["a", "b", "c"])]
async fn do_stuff() {}
```

### `docs`

External documentation link. URL is mandatory, description is optional.

Example:

```rust,ignore
#[handler(docs(
    url = "http://example.com/do_stuff",
    description = "Link title"
))]
async fn do_stuff() {}
```

### `deprecated`

Mark handler as deprecated in API specification.

Example:

```rust,ignore
#[handler(deprecated)]
async fn do_stuff() {}
```

### `layer`

Wrap the handler with custom `tower::Layer`.

Parameter should point to a function that returns said layer.

Example:

```rust,ignore
#[handler(layer = some_layer_fn)]
async fn do_stuff() {}
```

### `method`

HTTP request method to route into this handler.

Example:

```rust,ignore
#[handler(method = "PUT")]
async fn do_stuff() {}
```

Possible parameter values:

* `GET`
* `HEAD`
* `POST`
* `PUT`
* `DELETE`
* `OPTIONS`
* `TRACE`
* `PATCH`

When not specified, default is `POST` when request body extractor is present, otherwise default is `GET`.

### `name`

Unique name of a handler. Taken from function name if not specified as a parameter.

Example:

```rust,ignore
#[handler(name = "do_something")]
async fn do_stuff() {}
```

### `no_auth`

Do not require authentication to run this handler. Otherwise calling this handler will return HTTP 401/403
even when `permissions` parameter is not present.

Example:

```rust,ignore
#[handler(no_auth)]
async fn do_stuff() {}
```

### `path`

HTTP request path to route into this handler. Typically starts with `/`.

Example:

```rust,ignore
#[handler(path = "/path/to/method")]
async fn do_stuff() {}
```

When not specified, it is generated automatically by prepending `/` to handler name.

### `path_params`

Document and specify URL path template parameters.

Example:

```rust,ignore
#[handler(
    path = "/api/v1/{user}/{comment}"
    path_params(
        user(description = "User ID", value_type = "i32"),
        comment(description = "Comment ID", value_type = "i64", allow_empty = true)
    )
)]
async fn do_stuff() {}
```

Each path parameter can have following nested parameters inside:

* `allow_empty`: allow empty string to match parameter template.
* `description`: path parameter description.
* `deprecated`: parameter is deprecated.
* `value_type`: type to use when rendering JSON schema.

### `permissions`

List of required permissions. Each listed permission needs to be granted for handler to run.

Example:

```rust,ignore
#[handler(permissions = ["perm1", "perm2"])]
async fn do_stuff() {}
```

You can configure users, permissions and roles in configuration file, as long as you are using standard
config auth provider.

### `summary`

Short text describing the handler. Can be either written as a parameter:

```rust,ignore
#[handler(summary = "Summary text")]
async fn do_stuff() {}
```

Or alternatively it will be extracted from first line of doc string:

```rust,ignore
/// This line will be put into summary.
/// This line will not be put into summary.
#[handler]
async fn do_stuff() {}
```

### `tags`

List of tags assigned to the handler.

Example:

```rust,ignore
#[handler(tags = ["tag1", "tag2"])]
async fn do_stuff() {}
```

## Configuration reference

This section describes various configuration parameters present in [`ServiceConfig<C>`] structure.
Depending on how you initialize your configuration, you can source parameters from a file (either
YAML, TOML, JSON or any other format supported by `config` crate), import them from environment
variables, or modify specific parameters at runtime.

All configuration parameters are optional, and provide reasonable defaults. Default values are
described for each individual parameter below.

Time duration/interval values are parsed from human-readable format using [`humantime_serde`] crate.
For example: `5s`, `100ms` or `1h`.

Configuration example snippets are in YAML format.

> **Note**: This section is a rather terse placeholder documentation until a guidebook is ready.

### HTTP server configuration

This section configures embedded HTTP servers and their properties. You can define multiple servers
with same or different kinds. But be sure to make them not use conflicting hostnames/addresses and/or
ports in their listen strings.

Configuration section name: `servers`.

If no servers are provided, default settings take effect: one server element, listening on `localhost:8080`
using plain HTTP protocol, with all other server settings being the default.

This section is represented with a list (vector).
`x` within parameter names denotes index within `servers` section.

* `servers[x].http1.half_close`:
* `servers[x].http1.header_read_timeout`:
* `servers[x].http1.keepalive`:
* `servers[x].http1.max_buf_size`:
* `servers[x].http1.writev`:
* `servers[x].http2.adaptive_window`:
* `servers[x].http2.connect_protocol`:
* `servers[x].http2.initial_connection_window`:
* `servers[x].http2.initial_stream_window`:
* `servers[x].http2.keepalive.interval`:
* `servers[x].http2.keepalive.timeout`:
* `servers[x].http2.max_concurrent_streams`:

* `servers[x].ip.tos`: either IPv4 TOS or IPv6 traffic class field to use for outbound traffic.

  Note that parameter value is specified as a decimal number.

  No TOS/traffic class is set by default.

  Example:
  ```yaml
  servers:
    - ip:
        tos: 40
  ```

* `servers[x].kind`: type of protocol to use for incoming connections.

  Valid values:
  * `plain`: plain HTTP. Note that while HTTP/2 can be handled by this type, automatic protocol negotiation
    will not be performed. This is the default.
  * `tls`: HTTP over TLS, also known as HTTPS. Will negotiate protocols when not explicitly configured
    otherwise. mTLS support is currently unsupported, but planned for a future release.
  * `spiffe`: HTTP over mTLS, using SPIFFE certificate distribution and verification. Will negotiate
    protocols when not explicitly configured otherwise. SPIFFE IDs can be used as usernames when configuring
    RBAC/privileges, if SPIFFE auth extractor is enabled.

  Multiple servers of same type can be defined. Note that QUIC (HTTP/3) is currently unsupported.

  Example:
  ```yaml
  servers:
    - kind: plain
      listen: 127.0.0.1:8080
  ```

* `servers[x].listen`: hostname/address and port to listen on.

  You can use either use DNS hostnames or IPv4/IPv6 addresses to specify an address to bind to.
  Listening on UDP or UNIX sockets is currently unimplemented.

  Default value is `localhost:8080`.

  Example:
  ```yaml
  servers:
    - listen: 0.0.0.0:8888
  ```

  Be careful not to specify multiple server objects with conflicting address/port configurations.

* `servers[x].spiffe.alpn_protocols`: list of advertised protocols for TLS ALPN (protocol negotiation).

  Parameter is applicable only to server objects with `kind` = `spiffe`.

  See `servers[x].tls.alpn_protocols` for description of allowed parameter values.

  Default value is `[http2, http11]`.

  Example:
  ```yaml
  servers:
    - kind: spiffe
      spiffe:
        alpn_protocols:
          - http11
          - http2
  ```

* `servers[x].spiffe.authorize.spiffe_ids`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.authorize.trust_domains`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.authorize.type`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.handshake_timeout`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.initial_sync_timeout`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.limits.max_bundle_der_bytes`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.limits.max_bundles`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.limits.max_svids`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.reconnect.min_backoff`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.reconnect.max_backoff`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.shutdown_timeout`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.trust_domain_policy.domain`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.trust_domain_policy.domains`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.trust_domain_policy.type`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].spiffe.workload_api`:

  Parameter is applicable only to server objects with `kind` = `spiffe`.

* `servers[x].tcp.backlog`: max size of TCP backlog queue for listening socket.

  This queue holds newly accepted connections after a completed handshake, before they are received
  by the application.
  Various system settings might additionally cap this value.

  Uses system-specific default if not defined in configuration.

  Example:
  ```yaml
  servers:
    - tcp:
        backlog: 2048
  ```

* `servers[x].tcp.keepalive.idle`: idle period before we start sending TCP keepalive probes.

  Value is a duration in
  [`humantime_serde`](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html) format.

  TCP keepalive is disabled if value is not provided.

  Example:
  ```yaml
  servers:
    - tcp:
        keepalive:
          idle: 5m
  ```

* `servers[x].tcp.keepalive.interval`: interval between successive TCP keepalive probe retransmissions.

  Value is a duration in
  [`humantime_serde`](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html) format.

  Example:
  ```yaml
  servers:
    - tcp:
        keepalive:
          interval: 1s
  ```

* `servers[x].tcp.keepalive.retries`: number of unacknowledged TCP keepalive probes before we mark remote
  end as unavailable.

  Example:
  ```yaml
  servers:
    - tcp:
        keepalive:
          retries: 3
  ```

* `servers[x].tcp.mss`: maximum outgoing TCP segment size, in bytes.

  This modifies `TCP_MAXSEG` socket option.
  Value cannot be zero. Interface MTU might limit the effective MSS value.

  Example:
  ```yaml
  servers:
    - tcp:
        mss: 8000
  ```

* `servers[x].tcp.nodelay`: set `TCP_NODELAY` socket flag for listening socket.

  This disables Nagle algorithm, which minimizes delay before sending individual TCP segments.

  Default is `true`.

  Example:
  ```yaml
  servers:
    - tcp:
        nodelay: false
  ```

  Additional allowed names for this parameter: `no_delay`.

* `servers[x].tcp.recv_buffer`: size of socket receive buffer, in bytes.

  This modifies `SO_RCVBUF` socket option.
  Value cannot be zero. Moreover, different operating systems require different minimum sizes
  for this buffer.

  Uses system-specific default if not defined in configuration.

  Example:
  ```yaml
  servers:
    - tcp:
        recv_buffer: 262144
  ```

* `servers[x].tcp.send_buffer`: size of socket send buffer, in bytes.

  This modifies `SO_SNDBUF` socket option.
  Value cannot be zero. Moreover, different operating systems require different minimum sizes
  for this buffer.

  Uses system-specific default if not defined in configuration.

  Example:
  ```yaml
  servers:
    - tcp:
        send_buffer: 262144
  ```

* `servers[x].tls.alpn_protocols`: list of advertised protocols for TLS ALPN (protocol negotiation).

  Parameter is applicable only to server objects with `kind` = `tls`.

  Value must contain a list of one or more protocols:
  * `http11`: HTTP/1.1 (alternative names: `http`, `http1`, `http/1`, `http/1.1`, `h1`).
  * `http2`: HTTP/2 (alternative names: `http/2`, `h2`).

  Protocols are listed in the order of decreasing priority.

  Default value is `[http2, http11]`.

  Example:
  ```yaml
  servers:
    - kind: tls
      tls:
        alpn_protocols:
          - http11
          - http2
  ```

* `servers[x].tls.certificate`: local filesystem path to server certificate (chain) to use.

  Parameter is applicable only to server objects with `kind` = `tls`.

  File must contain X.509 server certificate, optionally with chain of signing certificates.
  File must use PEM encoding.

  There is no default; this is a required parameter for TLS servers.

  Example:
  ```yaml
  servers:
    - kind: tls
      tls:
        certificate: /path/to/some/cert.pem
  ```

  Additional allowed names for this parameter: `cert`, `chain`.

* `servers[x].tls.handshake_timeout`: maximum duration of TLS handshake process.

  Parameter is applicable only to server objects with `kind` = `tls`.

  Value is a duration in
  [`humantime_serde`](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html) format.

  Default value is 10 seconds.

  Example:
  ```yaml
  servers:
    - kind: tls
      tls:
        handshake_timeout: 1s
  ```

* `servers[x].tls.private_key`: local filesystem path to server private key.

  Parameter is applicable only to server objects with `kind` = `tls`.

  File must contain server private key, in either PKCS#1, PKCS#8 or SEC1 format.
  File must use PEM encoding.

  There is no default; this is a required parameter for TLS servers.

  Example:
  ```yaml
  servers:
    - kind: tls
      tls:
        private_key: /path/to/some/key.pem
  ```

  Additional allowed names for this parameter: `key`.

### Tokio runtime configuration

Configuration section name: `runtime`.

* `runtime.type`:
* `runtime.event_interval`:
* `runtime.global_unique_interval`:
* `runtime.max_blocking_threads`:
* `runtime.max_io_events_per_tick`:
* `runtime.thread_keep_alive`:
* `runtime.thread_name`:
* `runtime.thread_stack_size`:
* `runtime.worker_threads`:

### Generic OpenTelemetry configuration

Configuration section name: `telemetry`.

### Logging

Configuration section name: `logging`.

### Tracing

Configuration section name: `tracing`.

### Metrics

Configuration section name: `metrics`.

### Handler configuration

Configuration section name: `handlers`.

### API documentation

Configuration section name: `api_doc`.

### Probes and management API

Configuration section name: `probes`.

### Authentication and authorization

Configuration section name: `auth`.

### HTTP client settings

This section configures HTTP clients and their various features.
All configuration is contained in a hash map, with client name as a key.
Default values will be used if there is no configuration for a client used in your code.

Configuration section name: `http_clients`.

`XYZ` within parameter names denotes arbitrary client name (a text string).

* `http_clients.XYZ.kind`: type of client-side authentication to use.

  Valid values:
  * `plain`: no additional authentication. Will use regular ephemeral keys if TLS is required.
    This is the default.
  * `mtls`: provide client key and certificate for use in mTLS.
  * `spiffe`: HTTP over mTLS, using SPIFFE certificate distribution and verification. Will negotiate
    protocols via ALPN when not explicitly configured otherwise.

* `http_clients.XYZ.client_cert`
* `http_clients.XYZ.spiffe.limits.max_svids`
* `http_clients.XYZ.spiffe.limits.max_bundles`
* `http_clients.XYZ.spiffe.limits.max_bundle_der_bytes`
* `http_clients.XYZ.spiffe.workload_api`
* `http_clients.XYZ.spiffe.initial_sync_timeout`
* `http_clients.XYZ.spiffe.shutdown_timeout`
* `http_clients.XYZ.spiffe.reconnect.min_backoff`
* `http_clients.XYZ.spiffe.reconnect.max_backoff`
* `http_clients.XYZ.spiffe.authorize.type`
* `http_clients.XYZ.spiffe.authorize.spiffe_ids`
* `http_clients.XYZ.spiffe.authorize.trust_domains`
* `http_clients.XYZ.spiffe.trust_domain_policy.type`
* `http_clients.XYZ.spiffe.trust_domain_policy.domain`
* `http_clients.XYZ.spiffe.trust_domain_policy.domains`
* `http_clients.XYZ.spiffe.handshake_timeout`
* `http_clients.XYZ.spiffe.alpn_protocols`
* `http_clients.XYZ.connect_timeout`
* `http_clients.XYZ.read_timeout`
* `http_clients.XYZ.request_timeout`
* `http_clients.XYZ.pool_idle_timeout`
* `http_clients.XYZ.pool_max_idle_per_host`
* `http_clients.XYZ.verbose`
* `http_clients.XYZ.extra_headers.NAME`
* `http_clients.XYZ.redirect`
* `http_clients.XYZ.referer`
* `http_clients.XYZ.tcp.nodelay`
* `http_clients.XYZ.tcp.keepalive`
* `http_clients.XYZ.http2.adaptive_window`
* `http_clients.XYZ.http2.initial_connection_window`
* `http_clients.XYZ.http2.initial_stream_window`
* `http_clients.XYZ.http2.keepalive.interval`
* `http_clients.XYZ.http2.keepalive.timeout`
* `http_clients.XYZ.http2.keepalive.while_idle`
* `http_clients.XYZ.http2.max_frame_size`
* `http_clients.XYZ.cb.error_rate`
* `http_clients.XYZ.cb.closed_len`
* `http_clients.XYZ.cb.half_open_len`
* `http_clients.XYZ.cb.open_wait`
* `http_clients.XYZ.retry.kind`
* `http_clients.XYZ.retry.max_attempts`
* `http_clients.XYZ.retry.duration`
* `http_clients.XYZ.retry.min_backoff_duration`
* `http_clients.XYZ.retry.max_backoff_duration`
* `http_clients.XYZ.domain_overrides.NAME`
