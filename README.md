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
// 3. Write and async entry point.
//

use std::{net::SocketAddr, time::Duration};

/// # use uxum::prelude::*;

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
        .run(config.server, svc, Some(Duration::from_secs(5)))
        .await
        .map_err(Into::into)
}

//
// 4. Create your endpoint
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

## Handler parameters

This section describes all available parameters for a `#[handler]` procedural macro.

###

## Configuration reference

This section describes various configuration parameters present in [`ServiceConfig<C>`] structure.
Depending on how you initialize your configuration, you can source parameters from a file (either
YAML, TOML, JSON or any other format supported by `config` crate), import them from environment
variables, or modify specific parameters at runtime.

All configuration parameters are optional, and provide reasonable defaults. Default values are
described for each individual parameter below.

Time duration/interval values are parsed from human-readable format using [`humantime`] crate.
For example: `5s`, `100ms` or `1h`.

> **Note**: This section is a rather terse placeholder documentation until a guidebook is ready.

### HTTP server configuration

This section configures embedded HTTP servers and their properties. You can define multiple servers
with same or different kinds. But be sure to make them not use conflicting hostnames and/or ports in
their listen strings.

Configuration section name: `server`.

* `server.http1.half_close`:
* `server.http1.header_read_timeout`:
* `server.http1.keepalive`:
* `server.http1.max_buf_size`:
* `server.http1.writev`:
* `server.http2.adaptive_window`:
* `server.http2.connect_protocol`:
* `server.http2.initial_connection_window`:
* `server.http2.initial_stream_window`:
* `server.http2.keepalive.interval`:
* `server.http2.keepalive.timeout`:
* `server.http2.max_concurrent_streams`:
* `server.ip.tos`:
* `server.kind`:
* `server.listen`:
* `server.sleep_on_accept_errors`:
* `server.tcp.backlog`:
* `server.tcp.keepalive.idle`:
* `server.tcp.keepalive.interval`:
* `server.tcp.keepalive.retries`:
* `server.tls.listen`:
* `server.tcp.mss`:
* `server.tcp.nodelay`:
* `server.tcp.recv_buffer`:
* `server.tcp.send_buffer`:
* `server.tls.certificate`:
* `server.tls.private_key`:

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

Configuration section name: `http_clients`.
