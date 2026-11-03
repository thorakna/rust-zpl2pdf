//! Error type.

/// Errors that stop a conversion. Problems inside a label (unknown commands,
/// malformed parameters, bad barcode data) are reported as warnings instead.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The options are invalid (for example a zero sized label).
    #[error("invalid options: {0}")]
    InvalidOptions(String),
    /// The input contains no `^XA ... ^XZ` label.
    #[error("no label found in input")]
    NoLabels,
    /// The requested label index does not exist.
    #[error("label index {index} out of range ({count} labels)")]
    LabelIndex {
        /// Requested index.
        index: usize,
        /// Number of labels in the input.
        count: usize,
    },
    /// A resource limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(String),
    /// The PDF or PNG writer failed.
    #[error("output error: {0}")]
    Output(String),
}
