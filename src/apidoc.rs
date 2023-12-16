use std::collections::{BTreeMap, HashMap};

use askama::Template;
use axum::{
    extract::State,
    http::header,
    response::IntoResponse,
    routing::{self, Router},
    Extension,
};
use http::Method;
use okapi::{
    openapi3,
    schemars::gen::{SchemaGenerator, SchemaSettings},
    Map,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, debug_span};

use crate::builder::app::HandlerExt;

/// Error type used in API doc objects
#[derive(Debug, Error)]
pub enum ApiDocError {
    #[error(transparent)]
    RenderSpec(#[from] serde_json::Error),
    #[error("Method {0} not supported in OpenAPI spec")]
    UnsupportedMethod(Method),
}

/// Builder for API documentation spec and UI
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Template)]
#[template(path = "rapidoc.html.j2", ext = "html")]
pub struct ApiDocBuilder {
    /// URL path for API documentation UI (RapiDoc)
    #[serde(default = "ApiDocBuilder::default_apidoc_path")]
    apidoc_path: String,
    /// URL path for generated OpenAPI spec
    #[serde(default = "ApiDocBuilder::default_spec_path")]
    spec_path: String,
    /// URL path for embedded RapiDoc JavaScript source
    #[serde(default = "ApiDocBuilder::default_js_path")]
    js_path: String,
    /// Short app name
    #[serde(default)]
    app_name: Option<String>,
    /// App version
    #[serde(default)]
    app_version: Option<String>,
    /// App title for use in UI and documentation
    #[serde(default)]
    app_title: Option<String>,
    /// Top-level description
    #[serde(default)]
    description: Option<String>,
    /// Contact name
    #[serde(default)]
    contact_name: Option<String>,
    /// Contact URL
    #[serde(default)]
    contact_url: Option<String>,
    /// Contact email
    #[serde(default)]
    contact_email: Option<String>,
    /// Schema tag metadata
    #[serde(default)]
    tags: Vec<openapi3::Tag>,
    /// Whether to install RapiDoc UI endpoints
    #[serde(default = "crate::util::default_true")]
    enable_ui: bool,
    /// Inline the subschemas or use references
    ///
    /// See [`SchemaSettings::inline_subschemas`].
    #[serde(default)]
    inline_subschemas: bool,
    /// Attributes passed to RapiDoc component
    #[serde(default = "ApiDocBuilder::default_rapidoc_attributes")]
    rapidoc_attributes: HashMap<String, String>,
}

impl Default for ApiDocBuilder {
    fn default() -> Self {
        Self {
            apidoc_path: Self::default_apidoc_path(),
            spec_path: Self::default_spec_path(),
            js_path: Self::default_js_path(),
            app_name: None,
            app_version: None,
            app_title: None,
            description: None,
            contact_name: None,
            contact_url: None,
            contact_email: None,
            tags: vec![],
            enable_ui: true,
            inline_subschemas: false,
            rapidoc_attributes: Self::default_rapidoc_attributes(),
        }
    }
}

impl ApiDocBuilder {
    /// Hardcoded version of used OpenAPI specification
    const OPENAPI_VERSION: &'static str = "3.0.3";

    /// Default value for [`Self::apidoc_path`]
    #[must_use]
    fn default_apidoc_path() -> String {
        "/apidoc".into()
    }

    /// Default value for [`Self::spec_path`]
    #[must_use]
    fn default_spec_path() -> String {
        "/openapi.json".into()
    }

    /// Default value for [`Self::js_path`]
    #[must_use]
    fn default_js_path() -> String {
        "/rapidoc-min.js".into()
    }

    /// Default value for [`Self::rapidoc_attributes`]
    #[must_use]
    fn default_rapidoc_attributes() -> HashMap<String, String> {
        maplit::hashmap! {
            "sort-tags".into() => "true".into(),
            "theme".into() => "dark".into(),
            "layout".into() => "row".into(),
            "render-style".into() => "focused".into(),
            "allow-spec-file-download".into() => "true".into(),
            "schema-description-expanded".into() => "true".into(),
            "show-components".into() => "true".into(),
        }
    }

    /// Get app title as a string slice
    #[must_use]
    pub fn app_title(&self) -> &str {
        self.app_title
            .as_deref()
            .or(self.app_name.as_deref())
            .unwrap_or("Service")
    }

    /// Set URL path for API documentation UI (RapiDoc)
    #[must_use]
    pub fn with_apidoc_path(mut self, path: impl ToString) -> Self {
        self.apidoc_path = path.to_string();
        self
    }

    /// Set URL path for generated OpenAPI spec
    #[must_use]
    pub fn with_spec_path(mut self, path: impl ToString) -> Self {
        self.spec_path = path.to_string();
        self
    }

    /// Set URL path for embedded RapiDoc JavaScript source
    #[must_use]
    pub fn with_js_path(mut self, path: impl ToString) -> Self {
        self.js_path = path.to_string();
        self
    }

    /// Set short app name
    #[must_use]
    pub fn with_app_name(mut self, name: impl ToString) -> Self {
        self.app_name = Some(name.to_string());
        self
    }

    /// Set app version
    #[must_use]
    pub fn with_app_version(mut self, version: impl ToString) -> Self {
        self.app_version = Some(version.to_string());
        self
    }

    /// Set app title for use in UI and documentation
    #[must_use]
    pub fn with_app_title(mut self, title: impl ToString) -> Self {
        self.app_title = Some(title.to_string());
        self
    }

    /// Set top-level description
    #[must_use]
    pub fn with_description(mut self, descr: impl ToString) -> Self {
        self.description = Some(descr.to_string());
        self
    }

    /// Set contact name
    #[must_use]
    pub fn with_contact_name(mut self, name: impl ToString) -> Self {
        self.contact_name = Some(name.to_string());
        self
    }

    /// Set contact URL
    #[must_use]
    pub fn with_contact_url(mut self, url: impl ToString) -> Self {
        self.contact_url = Some(url.to_string());
        self
    }

    /// Set contact email
    #[must_use]
    pub fn with_contact_email(mut self, email: impl ToString) -> Self {
        self.contact_email = Some(email.to_string());
        self
    }

    /// Add optional information for a tag
    #[must_use]
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

    /// Disable RapiDoc UI
    #[must_use]
    pub fn without_ui(mut self) -> Self {
        self.enable_ui = false;
        self
    }

    /// Discourage use of references in generated OpenAPI schema
    #[must_use]
    pub fn with_inline_subschemas(mut self) -> Self {
        self.inline_subschemas = true;
        self
    }

    /// Set single RapiDoc attribute
    #[must_use]
    pub fn with_rapidoc_attribute<T, U>(mut self, key: T, value: U) -> Self
    where
        T: ToString,
        U: ToString,
    {
        self.rapidoc_attributes
            .insert(key.to_string(), value.to_string());
        self
    }

    /// Set multiple rapidoc attributes
    #[must_use]
    pub fn with_rapidoc_attributes<'a, T, U, V>(mut self, kvs: V) -> Self
    where
        T: ToString + 'a,
        U: ToString + 'a,
        V: IntoIterator<Item = (&'a T, &'a U)>,
    {
        self.rapidoc_attributes
            .extend(kvs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
        self
    }

    /// Set fallback app name and version
    ///
    /// This gets called from [`crate::AppBuilder`]
    pub fn set_app_defaults(&mut self, name: Option<String>, version: Option<String>) {
        if self.app_name.is_none() {
            self.app_name = name.map(|s| s.to_string());
        }
        if self.app_version.is_none() {
            self.app_version = version.map(|s| s.to_string());
        }
    }

    /// Create schema generator for custom types
    #[must_use]
    fn build_generator(&self) -> SchemaGenerator {
        SchemaSettings::openapi3()
            .with(|s| {
                s.inline_subschemas = self.inline_subschemas;
            })
            .into_generator()
    }

    /// Build Axum router containing all OpenAPI methods
    pub fn build_router(&self) -> Result<Router, ApiDocError> {
        let _span = debug_span!("build_apidoc").entered();
        let spec = self.render_spec()?;
        let mut rtr: Router = Router::new().route(
            &self.spec_path,
            routing::get(get_spec).layer(Extension(spec)),
        );
        if self.enable_ui {
            let js_map_path = format!("{}.map", &self.js_path);
            let index_path = format!("{}/index.html", &self.apidoc_path);
            rtr = rtr.merge(
                Router::new()
                    .route(&self.apidoc_path, routing::get(get_rapidoc_index))
                    .route(&index_path, routing::get(get_rapidoc_index))
                    .route(&self.js_path, routing::get(get_rapidoc_js))
                    .route(&js_map_path, routing::get(get_rapidoc_js_map))
                    .with_state(self.clone()),
            );
        }
        debug!("Built API doc router");
        Ok(rtr)
    }

    /// Build OpenAPI spec object hierarchy
    pub fn build_spec(&self) -> Result<openapi3::OpenApi, ApiDocError> {
        let _span = debug_span!("build_spec").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        for handler in inventory::iter::<&dyn HandlerExt> {
            grouped
                .entry(handler.spec_path())
                .and_modify(|handlers| handlers.push(*handler))
                .or_insert_with(|| vec![*handler]);
        }
        let mut gen = self.build_generator();
        let mut paths = Map::new();
        for (path, handlers) in grouped.into_iter() {
            // TODO: skip disabled handlers
            let mut path_item = openapi3::PathItem::default();
            for handler in handlers {
                let spec = handler.openapi_spec(&mut gen);
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
        let contact = if self.contact_name.is_some()
            || self.contact_email.is_some()
            || self.contact_url.is_some()
        {
            Some(openapi3::Contact {
                name: self.contact_name.clone(),
                url: self.contact_url.clone(),
                email: self.contact_email.clone(),
                extensions: Default::default(),
            })
        } else {
            None
        };
        Ok(openapi3::OpenApi {
            openapi: Self::OPENAPI_VERSION.into(),
            info: openapi3::Info {
                title: self.app_title().to_string(),
                description: self.description.clone(),
                terms_of_service: None,
                contact,
                license: None,
                version: self.app_version.clone().unwrap_or("0.0.0".into()),
                extensions: Default::default(),
            },
            servers: vec![],
            paths,
            components: Some(openapi3::Components {
                schemas: gen
                    .definitions()
                    .iter()
                    .map(|(key, schema)| (key.clone(), schema.clone().into_object()))
                    .collect(),
                responses: Default::default(),
                parameters: Default::default(),
                examples: Default::default(),
                request_bodies: Default::default(),
                headers: Default::default(),
                security_schemes: Default::default(),
                links: Default::default(),
                callbacks: Default::default(),
                extensions: Default::default(),
            }),
            security: vec![],
            tags: self.tags.clone(),
            external_docs: None,
            extensions: Default::default(),
        })
    }

    /// Build and serialize OpenAPI spec
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
async fn get_rapidoc_index(api_doc: State<ApiDocBuilder>) -> impl IntoResponse {
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
