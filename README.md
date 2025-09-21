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
 * `kafka`: support writing logs to a Kafka topic.
 * `systemd`: enable systemd integration for service notifications and watchdog support (Linux only).
