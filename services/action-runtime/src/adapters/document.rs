//! Document normalization: any source → FRIDAY [`Document`].
//!
//! AnyDoc (the `anydoc` crate, firecrawl's pure-Rust document parser) is the
//! backend for binary office formats — Word, PowerPoint, Excel, OpenDocument,
//! RTF, EPUB, CSV, PDF. Its internal representation never escapes this module:
//! every backend converges on the FRIDAY [`Document`] contract below. When an
//! OCR or VLM backend arrives it lands here too, behind the same shape.
//!
//! **No `DocumentParser` trait yet.** A second backend has not proven one needed
//! (the synthesis rule: don't invent an abstraction until a second
//! implementation demands it). The dispatch in [`parse_document`] is internal.
//!
//! Formats currently handled:
//! - binary/office → `anydoc` (detected by extension; AnyDoc re-detects from
//!   content and reports recovery events via the `log` facade)
//! - text/markdown (`.md`, `.txt`, `.rst`, `.adoc`) → passthrough
//! - anything else → explicit `unsupported` error naming the missing backend

use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Stable identifier for a parsed document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub String);

/// Where a document came from (path, URL, or attachment name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentSource(pub String);

impl DocumentSource {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Normalized document — the single downstream shape for every parser backend.
///
/// ```text
/// AnyDoc ──┐
/// OCR     ──┤→ Document → KnowledgeGraph → Memory → retrieval
/// VLM     ──┘
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub source: DocumentSource,
    pub title: Option<String>,
    pub mime_type: String,
    /// Clean GitHub-Flavored Markdown text (the retrieval/LLM-ready form).
    pub markdown: String,
    pub metadata: DocumentMetadata,
    /// Extracted binary assets (images, attachments). Empty today; populated by
    /// future OCR/VLM backends.
    pub assets: Vec<DocumentAsset>,
}

/// Provenance + parse facts about a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub filename: String,
    pub size_bytes: u64,
    /// Source format (extension without dot), e.g. `docx`, `markdown`.
    pub format: String,
    /// Which backend produced the markdown: `anydoc` or `passthrough`.
    pub parser: String,
    pub chars: usize,
    pub paragraphs: usize,
    pub parsed_at_ms: u64,
}

/// A binary asset extracted from a document (future OCR/VLM backends).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAsset {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub size_bytes: Option<u64>,
}

/// Text formats that are already readable and pass through untouched.
const TEXT_EXTS: &[&str] = &["md", "markdown", "txt", "rst", "adoc", "asciidoc"];

/// Parse a document file into the normalized FRIDAY [`Document`] contract.
pub fn parse_document(path: &Path) -> Result<Document, String> {
    if !path.is_file() {
        return Err(format!("not a file: {}", path.display()));
    }

    let bytes = std::fs::read(path).map_err(|e| format!("read failed: {}", e))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty());
    let size_bytes = bytes.len() as u64;

    let (markdown, parser, mime_type) = if let Some(_fmt) = anydoc::Format::from_extension(&ext) {
        let md = anydoc::to_markdown(path)
            .map_err(|e| format!("anydoc conversion failed for {}: {:?}", path.display(), e))?;
        (md, "anydoc", mime_for(&ext))
    } else if TEXT_EXTS.contains(&ext.as_str()) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("not UTF-8 text: {}", path.display()))?;
        (text.to_string(), "passthrough", mime_for(&ext))
    } else {
        return Err(format!(
            "unsupported document format `{ext}` for docs.parse (anydoc handles \
             doc/docx/docm/odt/pdf/ppt/pptx/rtf/epub/xls/xlsx/xlsm/xlsb/ods/odp/csv; \
             md/txt/rst/adoc pass through; OCR/VLM backends not wired)"
        ));
    };

    let paragraphs = markdown
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .count();
    let chars = markdown.chars().count();

    Ok(Document {
        id: DocumentId(Uuid::new_v4().simple().to_string()),
        source: DocumentSource(path.to_string_lossy().to_string()),
        title,
        mime_type: mime_type.to_string(),
        markdown,
        metadata: DocumentMetadata {
            filename,
            size_bytes,
            format: ext.clone(),
            parser: parser.to_string(),
            chars,
            paragraphs,
            parsed_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        },
        assets: Vec::new(),
    })
}

/// MIME type for a known source extension.
fn mime_for(ext: &str) -> &'static str {
    match ext {
        "docx" | "docm" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        "doc" => "application/msword",
        "odt" => "application/vnd.oasis.opendocument.text",
        "pdf" => "application/pdf",
        "pptx" | "pptm" | "ppsx" | "ppsm" | "pot" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        "ppt" => "application/vnd.ms-powerpoint",
        "rtf" => "application/rtf",
        "epub" => "application/epub+zip",
        "xlsx" | "xlsm" | "xlsb" => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "csv" => "text/csv",
        "md" | "markdown" => "text/markdown",
        "rst" => "text/x-rst",
        "adoc" | "asciidoc" => "text/asciidoc",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mime_mapping_covers_supported_formats() {
        assert!(mime_for("docx").starts_with("application/"));
        assert_eq!(mime_for("md"), "text/markdown");
        assert_eq!(mime_for("csv"), "text/csv");
        assert_eq!(mime_for("nope"), "application/octet-stream");
    }

    #[test]
    fn parses_docx_to_markdown() {
        let dir = temp_tree();
        let path = dir.path().join("spec.docx");
        std::fs::write(
            &path,
            minimal_docx(&[
                "FRIDAY supports DOCX parsing.",
                "AnyDoc converts documents to Markdown.",
            ]),
        )
        .unwrap();

        let doc = parse_document(&path).unwrap();
        assert!(doc.markdown.contains("FRIDAY supports DOCX parsing."));
        assert!(doc
            .markdown
            .contains("AnyDoc converts documents to Markdown."));
        assert_eq!(doc.metadata.parser, "anydoc");
        assert_eq!(doc.metadata.format, "docx");
        assert!(doc.mime_type.starts_with("application/"));
        assert_eq!(doc.title.as_deref(), Some("spec"));
        assert_eq!(doc.metadata.paragraphs, 2);
        assert!(doc.assets.is_empty());
    }

    #[test]
    fn passes_markdown_through_untouched() {
        let dir = temp_tree();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "# Notes\n\nAlready markdown.\n").unwrap();

        let doc = parse_document(&path).unwrap();
        assert_eq!(doc.markdown, "# Notes\n\nAlready markdown.\n");
        assert_eq!(doc.metadata.parser, "passthrough");
        assert_eq!(doc.mime_type, "text/markdown");
    }

    #[test]
    fn rejects_unsupported_formats() {
        let dir = temp_tree();
        let path = dir.path().join("mystery.xyz");
        std::fs::write(&path, b"not a real document").unwrap();

        let err = parse_document(&path).unwrap_err();
        assert!(
            err.contains("unsupported document format `xyz`"),
            "err: {err}"
        );
    }

    #[test]
    fn rejects_missing_file() {
        let dir = temp_tree();
        let err = parse_document(&dir.path().join("absent.docx")).unwrap_err();
        assert!(err.contains("not a file"), "err: {err}");
    }

    /// Build a minimal (valid) `.docx` in memory for tests.
    fn minimal_docx(paragraphs: &[&str]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();

            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(
                b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>",
            )
            .unwrap();

            zip.start_file("_rels/.rels", opts).unwrap();
            zip.write_all(
                b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>",
            )
            .unwrap();

            zip.start_file("word/document.xml", opts).unwrap();
            let body: String = paragraphs
                .iter()
                .map(|p| {
                    let esc = p
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    format!("<w:p><w:r><w:t>{esc}</w:t></w:r></w:p>")
                })
                .collect();
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:body>{body}</w:body></w:document>"
            );
            zip.write_all(xml.as_bytes()).unwrap();

            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    fn temp_tree() -> tempfile_lite::TempDir {
        tempfile_lite::TempDir::new().expect("temp dir")
    }
}

/// Minimal RAII temp dir for tests (avoids adding a dev-dependency).
#[cfg(test)]
mod tempfile_lite {
    use std::path::PathBuf;

    pub struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub fn new() -> std::io::Result<Self> {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "friday-document-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
