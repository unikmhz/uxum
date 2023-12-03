use askama::Template;
use axum::{http::header, response::IntoResponse};

/// Builder for API documentation spec and UI.
#[derive(Debug, Template)]
#[template(path = "rapidoc.html.j2", ext = "html")]
pub struct ApiDocBuilder {
    apidoc_path: String,
    spec_path: String,
    js_path: String,
    service_name: Option<String>,
}

impl Default for ApiDocBuilder {
    fn default() -> Self {
        Self {
            apidoc_path: "/apidoc".into(),
            spec_path: "/openapi.json".into(),
            js_path: "/rapidoc-min.js".into(),
            service_name: None,
        }
    }
}

impl ApiDocBuilder {
    ///
    pub fn with_apidoc_path(mut self, path: impl ToString) -> Self {
        self.apidoc_path = path.to_string();
        self
    }

    ///
    pub fn with_spec_path(mut self, path: impl ToString) -> Self {
        self.spec_path = path.to_string();
        self
    }

    ///
    pub fn with_js_path(mut self, path: impl ToString) -> Self {
        self.js_path = path.to_string();
        self
    }
}

pub async fn rapidoc_index() -> impl IntoResponse {
    // TODO: get from configuration
    ApiDocBuilder::default()
}

pub async fn rapidoc_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_bytes!("../static/rapidoc-min.js").as_slice(),
    )
}

pub async fn rapidoc_js_map() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        include_bytes!("../static/rapidoc-min.js.map").as_slice(),
    )
}
