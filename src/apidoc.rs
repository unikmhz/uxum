use std::collections::BTreeMap;

use askama::Template;
use axum::{
    http::header,
    response::IntoResponse,
    routing::{self, Router},
    Extension,
};
use http::Method;
use okapi::{openapi3, Map};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, debug_span};

use crate::builder::app::HandlerExt;

///
#[derive(Debug, Error)]
pub enum ApiDocError {
    #[error(transparent)]
    RenderSpec(#[from] serde_json::Error),
    #[error("Method {0} not supported in OpenAPI spec")]
    UnsupportedMethod(Method),
}

/// Builder for API documentation spec and UI.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Template)]
#[template(path = "rapidoc.html.j2", ext = "html")]
pub struct ApiDocBuilder {
    ///
    apidoc_path: String,
    ///
    spec_path: String,
    ///
    js_path: String,
    ///
    app_name: Option<String>,
    ///
    app_version: Option<String>,
    ///
    app_title: Option<String>,
    ///
    tags: Vec<openapi3::Tag>,
}

impl Default for ApiDocBuilder {
    fn default() -> Self {
        Self {
            apidoc_path: "/apidoc".into(),
            spec_path: "/openapi.json".into(),
            js_path: "/rapidoc-min.js".into(),
            app_name: None,
            app_version: None,
            app_title: None,
            tags: vec![],
        }
    }
}

impl ApiDocBuilder {
    const OPENAPI_VERSION: &'static str = "3.0.3";

    ///
    pub fn app_title(&self) -> &str {
        self.app_title
            .as_deref()
            .or(self.app_name.as_deref())
            .unwrap_or("Service")
    }

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

    ///
    pub fn with_app_name(mut self, name: impl ToString) -> Self {
        self.app_name = Some(name.to_string());
        self
    }

    ///
    pub fn with_app_version(mut self, version: impl ToString) -> Self {
        self.app_version = Some(version.to_string());
        self
    }

    ///
    pub fn with_app_title(mut self, title: impl ToString) -> Self {
        self.app_title = Some(title.to_string());
        self
    }

    pub fn with_tag<T, U, V>(mut self, tag: T, description: Option<U>, url: Option<V>) -> Self
    where
        T: ToString,
        U: ToString,
        V: ToString,
    {
        self.tags.push(openapi3::Tag {
            name: tag.to_string(),
            external_docs: url.as_ref().map(|u| openapi3::ExternalDocs {
                description: description.as_ref().map(|d| d.to_string()),
                url: u.to_string(),
                extensions: Default::default(),
            }),
            description: description.map(|d| d.to_string()),
            extensions: Default::default(),
        });
        self
    }

    ///
    pub fn set_app_defaults(&mut self, name: Option<String>, version: Option<String>) {
        if self.app_name.is_none() {
            self.app_name = name.map(|s| s.to_string());
        }
        if self.app_version.is_none() {
            self.app_version = version.map(|s| s.to_string());
        }
    }

    ///
    pub fn build_router(&self) -> Result<Router, ApiDocError> {
        let _span = debug_span!("build_apidoc").entered();
        let js_map_path = format!("{}.map", &self.js_path);
        let index_path = format!("{}/index.html", &self.apidoc_path);
        let spec = self.render_spec()?;
        let rtr = Router::new()
            .route(
                &self.apidoc_path,
                routing::get(get_rapidoc_index).layer(Extension(self.clone())),
            )
            .route(
                &self.spec_path,
                routing::get(get_spec).layer(Extension(spec)),
            )
            .route(
                &index_path,
                routing::get(get_rapidoc_index).layer(Extension(self.clone())),
            )
            .route(&self.js_path, routing::get(get_rapidoc_js))
            .route(&js_map_path, routing::get(get_rapidoc_js_map));
        debug!("Built API doc router");
        Ok(rtr)
    }

    ///
    pub fn build_spec(&self) -> Result<openapi3::OpenApi, ApiDocError> {
        let _span = debug_span!("build_spec").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        for handler in inventory::iter::<&dyn HandlerExt> {
            grouped
                .entry(handler.path())
                .and_modify(|handlers| handlers.push(*handler))
                .or_insert_with(|| vec![*handler]);
        }
        let mut paths = Map::new();
        for (path, handlers) in grouped.into_iter() {
            let mut path_item = openapi3::PathItem {
                reference: None,
                summary: None,
                description: None,
                get: None,
                put: None,
                post: None,
                delete: None,
                options: None,
                head: None,
                patch: None,
                trace: None,
                servers: None,
                parameters: vec![],
                extensions: Default::default(),
            };
            for handler in handlers {
                let spec = handler.openapi_spec();
                match handler.method() {
                    Method::GET => path_item.get = Some(spec),
                    Method::PUT => path_item.put = Some(spec),
                    Method::POST => path_item.post = Some(spec),
                    Method::DELETE => path_item.delete = Some(spec),
                    Method::OPTIONS => path_item.options = Some(spec),
                    Method::HEAD => path_item.head = Some(spec),
                    Method::PATCH => path_item.patch = Some(spec),
                    Method::TRACE => path_item.trace = Some(spec),
                    other => return Err(ApiDocError::UnsupportedMethod(other)),
                }
            }
            let path = path.to_string();
            paths.insert(path, path_item);
        }
        Ok(openapi3::OpenApi {
            openapi: Self::OPENAPI_VERSION.into(),
            info: openapi3::Info {
                title: self.app_title().to_string(),
                description: None,
                terms_of_service: None,
                contact: None,
                license: None,
                version: self.app_version.clone().unwrap_or("0.0.0".into()),
                extensions: Default::default(),
            },
            servers: vec![],
            paths,
            components: None,
            security: vec![],
            tags: self.tags.clone(),
            external_docs: None,
            extensions: Default::default(),
        })
    }

    ///
    pub fn render_spec(&self) -> Result<OpenApiSpec, ApiDocError> {
        serde_json::to_vec_pretty(&self.build_spec()?)
            .map(OpenApiSpec)
            .map_err(Into::into)
    }
}

///
#[derive(Clone)]
#[repr(transparent)]
pub struct OpenApiSpec(Vec<u8>);

///
async fn get_spec(spec: Extension<OpenApiSpec>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/swagger+json")],
        spec.0 .0,
    )
}

///
async fn get_rapidoc_index(api_doc: Extension<ApiDocBuilder>) -> impl IntoResponse {
    api_doc.0.into_response()
}

///
async fn get_rapidoc_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_bytes!("../static/rapidoc-min.js").as_slice(),
    )
}

///
async fn get_rapidoc_js_map() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        include_bytes!("../static/rapidoc-min.js.map").as_slice(),
    )
}
