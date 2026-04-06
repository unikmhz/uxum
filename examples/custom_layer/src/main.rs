//! Example of using OAuth2/JWT authentication with uxum.

use aliri::{jwt, Jwks};
use aliri_oauth2::Authority;
use aliri_tower::{self, Oauth2Authorizer};
use axum::body::Body;
use tower_http::validate_request::{ValidateRequest, ValidateRequestHeaderLayer};
use uxum::{
    prelude::*,
    reexport::{tokio, tower, tracing_subscriber},
};

/// Application entry point.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let app_builder = AppBuilder::default();
    let app = app_builder.build().expect("Unable to build app");
    ServerBuilder::new()
        .build()
        .await
        .expect("Unable to build server")
        .serve(app.into_make_service())
        .await
        .expect("Server error");
}

fn jwt_auth_layer(
) -> ValidateRequestHeaderLayer<impl ValidateRequest<Body, ResponseBody = Body> + Clone> {
    let validator = jwt::CoreValidator::default();
    let authority = Authority::new(Jwks::default(), validator);
    let authorizer = Oauth2Authorizer::new().with_terse_error_handler::<axum::body::Body>();

    authorizer.jwt_layer(authority)
}

/// Protected endpoint - requires valid JWT token
#[handler(
    name = "protected",
    path = "/protected",
    method = "GET",
    layer = jwt_auth_layer
)]
async fn protected_handler() -> &'static str {
    "This is a protected endpoint - you have a valid JWT token!"
}
