use darling::FromMeta;
use syn::Path;

///
#[derive(Clone, Debug, Default, FromMeta)]
pub(crate) struct OpenApiPathParameter {
    ///
    #[darling(default)]
    pub(crate) description: Option<String>,
    ///
    #[darling(default)]
    pub(crate) deprecated: bool,
    ///
    #[darling(default)]
    pub(crate) allow_empty: bool,
    ///
    #[darling(default)]
    pub(crate) value_type: Option<Path>,
}
