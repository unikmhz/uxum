# UXUM

An opinionated backend service framework based on axum

## Project goals

 * Minimum boilerplate code.
 * Minimal performance impact from features not in use.
 * Metrics, tracing, OpenAPI and common service support features available out of the box.
 * Ready to be deployed on a local server, VM or container, on the cloud.

## Project non-goals

 * Performance and feature parity with bare axum.
   Straight-up axum without all bells and whistles provided by this framework will always be a bit faster
   and more flexible.
 * Database access layers and connection pools.
   This is out of scope for this project.
