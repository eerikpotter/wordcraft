//! Browser host adapter: the editor retains ownership of its native document format.
use sovereign_cloud_ui::{DocumentState, Editor, Snapshot};

pub struct Adapter<'a>(pub &'a mut wordcraft_ui_egui::WordApp);
impl Editor for Adapter<'_> {
    fn document(&self) -> Option<DocumentState> {
        let s = &self.0.session;
        Some(DocumentState {
            id: s.document_id(),
            revision: s.rev(),
            name: if s.doc.core.title.is_empty() { "Untitled".into() } else { s.doc.core.title.clone() },
            dirty: s.dirty,
        })
    }
    fn capture(&self) -> Result<Snapshot, String> {
        let document = self.document().ok_or("No document is open")?;
        let doc = self.0.session.doc.clone();
        Ok(Snapshot { document, encode: Box::new(move || wordcraft_engine::native::encode_bounded(&doc, 64 * 1024 * 1024)) })
    }
    fn saved(&mut self, id: u64, revision: u64, _bytes: &[u8]) -> Result<Option<u64>, String> {
        if self.document().is_some_and(|d| d.id == id && d.revision == revision) {
            self.0.session.dirty = false;
            return Ok(Some(revision));
        }
        Ok(None)
    }
    fn open(&mut self, _name: &str, bytes: &[u8]) -> Result<DocumentState, String> {
        let doc = wordcraft_engine::native::decode_bounded(bytes, 64 * 1024 * 1024)?;
        self.0.session.set_document(doc);
        self.0.session.path = None;
        self.0.ui.backstage = false;
        self.0.canvas.want_focus = true;
        self.document().ok_or_else(|| "Could not open the document".into())
    }
    fn replaces_unsaved(&self) -> bool {
        self.0.session.dirty
    }
}
