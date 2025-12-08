//! Resource selector configuration.

use std::collections::HashSet;

use opentelemetry::Key;

/// `ResourceSelector` is used to select which resource to export with every
/// metrics.
///
/// By default, the exporter will only export resource as `target_info` metrics
/// but not inline in every metrics. You can disable this behavior by calling
/// [`without_target_info`](crate::exporter::ExporterBuilder::without_target_info)
///
/// You can add resource to every metrics by set `ResourceSelector` to anything
/// other than `None`.
///
/// By default, `ResourceSelector` is `None`, meaning resource will not be
/// attributes of every metrics.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub enum ResourceSelector {
    /// Export all resource attributes with every metrics.
    All,
    /// Do not export any resource attributes with every metrics.
    #[default]
    None,
    /// Export only the resource attributes in the allow list with every
    /// metrics.
    KeyAllowList(HashSet<Key>),
}

impl From<HashSet<Key>> for ResourceSelector {
    fn from(keys: HashSet<Key>) -> Self {
        ResourceSelector::KeyAllowList(keys)
    }
}

impl From<Key> for ResourceSelector {
    fn from(value: Key) -> Self {
        let mut allow_list = HashSet::new();
        allow_list.insert(value);
        ResourceSelector::KeyAllowList(allow_list)
    }
}

impl From<bool> for ResourceSelector {
    fn from(value: bool) -> Self {
        if value {
            ResourceSelector::All
        } else {
            ResourceSelector::None
        }
    }
}

impl ResourceSelector {
    #[inline]
    #[must_use]
    pub(crate) fn matches(&self, key: &Key) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::KeyAllowList(list) => list.contains(key),
        }
    }
}
