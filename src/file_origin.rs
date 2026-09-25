//! Structured provenance for virtual files
//!
//! Some files registered in a [`SourceContext`](crate::SourceContext) are
//! *virtual*: their content was extracted from another file — a notebook
//! cell from a `.ipynb`, a region of a larger document. [`FileOrigin`]
//! records that relationship on the file's
//! [`FileMetadata`](crate::FileMetadata) so consumers can address the
//! position the author knows (the owning cell), hyperlink the real file
//! on disk, and emit structured location data — instead of encoding all
//! of that into a synthetic file path.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Where a virtual file's content really comes from.
///
/// Attached to a file via `FileMetadata::origin`. The canonical display
/// label (via [`fmt::Display`]) is what diagnostics show the user; the
/// structured fields are what JSON consumers get.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FileOrigin {
    /// Content extracted from one cell of a computational notebook.
    NotebookCell {
        /// Path of the notebook file on disk — the hyperlink target and
        /// the `file` reported in structured output.
        notebook_path: String,
        /// 1-based index of the cell within the notebook, counted over
        /// all cells regardless of type.
        cell_index: usize,
        /// The cell's `id` field (nbformat ≥ 4.5), when present.
        /// Displayed in structured output only, never in text labels.
        cell_id: Option<String>,
        /// The nbformat cell type (`"code"`, `"markdown"`, `"raw"`).
        cell_type: String,
    },
}

impl fmt::Display for FileOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileOrigin::NotebookCell {
                notebook_path,
                cell_index,
                cell_type,
                ..
            } => write!(f, "{notebook_path}[cell {cell_index}, {cell_type}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> FileOrigin {
        FileOrigin::NotebookCell {
            notebook_path: "notebook.ipynb".into(),
            cell_index: 2,
            cell_id: Some("abc123".into()),
            cell_type: "markdown".into(),
        }
    }

    #[test]
    fn display_is_the_cell_qualified_label() {
        assert_eq!(origin().to_string(), "notebook.ipynb[cell 2, markdown]");
    }

    #[test]
    fn display_omits_cell_id() {
        // cell.id is structured-output-only; the text label must not
        // change when a notebook gains or loses nbformat 4.5 ids.
        let no_id = FileOrigin::NotebookCell {
            notebook_path: "notebook.ipynb".into(),
            cell_index: 2,
            cell_id: None,
            cell_type: "markdown".into(),
        };
        assert_eq!(no_id.to_string(), origin().to_string());
    }

    #[test]
    fn serde_round_trip_keeps_all_fields() {
        let json = serde_json::to_string(&origin()).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"notebook_cell","notebook_path":"notebook.ipynb","cell_index":2,"cell_id":"abc123","cell_type":"markdown"}"#
        );
        let back: FileOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, origin());
    }

    #[test]
    fn serde_round_trip_without_cell_id() {
        let o = FileOrigin::NotebookCell {
            notebook_path: "n.ipynb".into(),
            cell_index: 7,
            cell_id: None,
            cell_type: "code".into(),
        };
        let back: FileOrigin = serde_json::from_str(&serde_json::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
    }
}
