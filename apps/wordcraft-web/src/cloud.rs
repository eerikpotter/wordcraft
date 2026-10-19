//! Browser host adapter: the editor retains ownership of its native document format.
use sovereign_cloud_ui::{DocumentState, Editor, Snapshot};

pub struct Adapter<'a>(pub &'a mut wordcraft_ui_egui::WordApp);
impl Editor for Adapter<'_> {
    fn document(&self) -> Option<DocumentState> {
        if !self.0.has_document() {
            return None;
        }
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
        self.0.has_document() && self.0.session.dirty
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use wordcraft_doc::Document;
    use wordcraft_engine::Session;
    use wordcraft_ui_egui::{Services, WordApp};

    #[test]
    fn start_screen_is_not_a_document_and_cloud_open_activates_one() {
        let mut app = WordApp::new(Session::new(Document::new()), Services::default());
        assert!(app.start_screen());
        let mut adapter = Adapter(&mut app);
        assert!(adapter.document().is_none());
        assert!(adapter.capture().is_err());
        assert!(!adapter.replaces_unsaved());
        let bytes = wordcraft_engine::native::encode_bounded(&Document::from_text("Cloud document"), 1024 * 1024).unwrap();
        let opened = adapter.open("Example.wcraft.json", &bytes).unwrap();
        assert!(adapter.0.has_document());
        assert!(!adapter.0.ui.backstage);
        assert_eq!((adapter.capture().unwrap().encode)().unwrap(), bytes);
        adapter.0.session.dirty = true;
        assert!(adapter.replaces_unsaved());
        assert_eq!(adapter.saved(opened.id, opened.revision, &bytes).unwrap(), Some(opened.revision));
        adapter.0.session.set_document(Document::new());
        adapter.0.session.dirty = true;
        assert_eq!(adapter.saved(opened.id, opened.revision, &bytes).unwrap(), None);
        assert!(adapter.replaces_unsaved());
        let current = adapter.document().unwrap().id;
        assert!(adapter.open("Broken", b"not a document").is_err());
        assert_eq!(adapter.document().unwrap().id, current);
    }
}
