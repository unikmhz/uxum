# UXUM

An opinionated backend service framework based on axum

## Project goals

 * Minimum boilerplate code.
 * Minimal performance impact from features not in use.
 * Metrics, tracing, OpenAPI and common service support features available out of the box.

## Project non-goals

 * Performance and feature parity with bare axum.
   Straight-up axum without all bells and whistles this framework provides will always be a bit faster
   and more flexible.
 * HTTP clients and databases accesses.
   This is out of scope for this project.
