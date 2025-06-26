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
 * `systemd`: enable systemd integration for service notifications and watchdog support (Linux only).
