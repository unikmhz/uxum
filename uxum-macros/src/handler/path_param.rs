use darling::FromMeta;
use syn::Path;

/// Path parameter schema
#[derive(Clone, Debug, Default, FromMeta)]
pub(crate) struct OpenApiPathParameter {
    /// Description
    #[darling(default)]
    pub(crate) description: Option<String>,
    /// Deprecation flag
    #[darling(default)]
    pub(crate) deprecated: bool,
    /// Allow empty value
    #[darling(default)]
    pub(crate) allow_empty: bool,
    /// Type of parameter value
    #[darling(default)]
    pub(crate) value_type: Option<Path>,
}
