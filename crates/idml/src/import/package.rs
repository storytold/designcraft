use super::{IdmlError, Result};

/// An IDML package reader backed by the shared bounded package implementation.
pub(super) struct Package<'a> {
    inner: craft_idml::Package<'a>,
}

impl<'a> Package<'a> {
    pub(super) fn open(bytes: &'a [u8]) -> Result<Self> {
        craft_idml::Package::open(bytes).map(|inner| Package { inner }).map_err(|e| IdmlError::NotIdml(e.to_string()))
    }

    /// Reads the designmap, XML parts, included resources and metadata with the shared parser.
    pub(super) fn document(&mut self) -> Result<craft_idml::Document> {
        self.inner.document().map_err(|e| match e {
            craft_idml::Error::TooLarge { part, limit } => IdmlError::Part { part, msg: format!("larger than the {limit}-byte package read limit") },
            craft_idml::Error::Part { part, msg } => IdmlError::Part { part, msg },
            other => IdmlError::NotIdml(other.to_string()),
        })
    }
}

#[cfg(test)]
pub(crate) use craft_idml::value::percent_decode;
pub(crate) use craft_idml::value::uri_to_path;
