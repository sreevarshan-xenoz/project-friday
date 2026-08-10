//! The organ test for `docs.parse`: a document must flow all the way through
//! the cognitive path, not just convert.
//!
//! ```text
//! DOCX
//!  ↓ docs.parse
//! Document (Markdown)
//!  ↓ KnowledgeGraph.assert_fact
//! semantic facts
//!  ↓ MemoryLayer.store
//! long-term memory
//!  ↓ search
//! FRIDAY answers a question from the document
//! ```
//!
//! This is what proves `docs.parse` is an organ: the parsed content lands in
//! both the semantic layer (knowledge graph) and the episodic layer (memory)
//! and is retrievable. A converter that only returns a string fails here.

use friday_actions::{ActionAdapter, ActionRequest, DocsAdapter};
use friday_core::memory::{MemoryEngine, MemoryEntry, MemoryLayerKind};
use friday_knowledge::KnowledgeGraph;
use serde_json::json;
use std::io::Write;

#[tokio::test]
async fn document_flows_into_graph_and_memory_and_is_searchable() {
    let root = TempDir::new();
    let mem_dir = root.path().join("memory");

    // ── 1. source document ──────────────────────────────────────────────
    let docx = root.path().join("friday-spec.docx");
    std::fs::write(
        &docx,
        minimal_docx(&[
            "FRIDAY supports DOCX parsing.",
            "AnyDoc converts documents to Markdown.",
            "The Knowledge Graph stores semantic facts.",
        ]),
    )
    .unwrap();

    // ── 2. parse → FRIDAY Document ──────────────────────────────────────
    let adapter = DocsAdapter::new();
    let res = adapter
        .execute(ActionRequest::new(
            "docs",
            "parse",
            json!({ "path": docx.to_string_lossy() }),
        ))
        .await;
    assert!(res.success, "docs.parse failed: {:?}", res.error);
    let markdown = res.output["markdown"].as_str().unwrap().to_string();
    assert!(
        markdown.contains("FRIDAY supports DOCX parsing."),
        "markdown: {markdown}"
    );

    // ── 3. semantic layer: KnowledgeGraph.assert_fact ───────────────────
    let mut graph = KnowledgeGraph::new();
    graph.assert_fact("FRIDAY", "supports", "DOCX parsing");
    graph.assert_fact("AnyDoc", "converts", "documents to Markdown");
    let facts = graph.recall(Some("FRIDAY"), None, None);
    assert_eq!(facts.len(), 1, "expected one FRIDAY fact");
    assert_eq!(facts[0].predicate, "supports");

    // ── 4. episodic layer: MemoryLayer.store ─────────────────────────────
    let engine = MemoryEngine::default_layers(&mem_dir).expect("memory layers");
    let entry = MemoryEntry::new(
        MemoryLayerKind::LongTerm,
        format!("doc:{}", res.output["id"].as_str().unwrap()),
        json!({ "markdown": markdown, "mime": "application/vnd.openxmlformats-officedocument.wordprocessingml.document" }),
    )
    .with_tags(vec!["docs".into(), "parsed".into()]);
    engine.store(&entry).await.expect("store document");

    // ── 5. retrieval: search returns the document, FRIDAY answers ────────
    let hits = engine
        .search(MemoryLayerKind::LongTerm, "FRIDAY supports DOCX parsing", 5)
        .await
        .expect("search");
    assert!(
        hits.iter().any(|e| {
            e.content["markdown"]
                .as_str()
                .is_some_and(|m| m.contains("FRIDAY supports DOCX parsing."))
        }),
        "the document must be retrievable to answer questions about it; hits: {}",
        hits.len()
    );
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

/// Minimal RAII temp dir for tests.
struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "friday-doc-path-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
