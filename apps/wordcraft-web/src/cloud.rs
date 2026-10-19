//! Browser host adapter: the editor retains ownership of its native document format.
use sovereign_cloud_ui::{DocumentState, Editor, Snapshot};

pub struct Adapter<'a>(pub &'a mut wordcraft_ui_egui::WordApp);
impl Editor for Adapter<'_> {
    fn create_document(&mut self) -> Result<(), String> {
        if self.document().is_some() {
            return Ok(());
        }
        self.0.run("file.new", serde_json::json!({}))?;
        self.0.ui.backstage = false;
        self.0.canvas.want_focus = true;
        Ok(())
    }
    fn set_user_profile(&mut self, profile: Option<sovereign_cloud_ui::UserProfile<'_>>) {
        let previous = self.0.session.user_id.is_some();
        self.0.session.user_id = profile.map(|p| p.id.to_owned());
        if profile.is_some() || previous {
            self.0.session.initials.clear();
        }
        if let Some(profile) = profile {
            if self.0.session.author != profile.display_name {
                self.0.session.author = profile.display_name.to_owned();
            }
        } else if previous {
            self.0.session.author = "WordCraft User".into();
        }
    }

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

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn existing_comments_and_replies_use_subnet_names_and_roundtrip() {
        use serde_json::json;
        use sovereign_cloud_ui::UserProfile;
        let mut app = wordcraft_ui_egui::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), Default::default());
        app.session.set_document(wordcraft_doc::Document::from_text("Shared document"));
        Adapter(&mut app).set_user_profile(Some(UserProfile { id: "account-a", display_name: "Alice Subnet" }));
        let root = app.session.run("review.newComment", &json!({"text":"Please review"})).unwrap()["id"].as_u64().unwrap() as u32;
        Adapter(&mut app).set_user_profile(Some(UserProfile { id: "account-b", display_name: "Bob Subnet" }));
        let reply = app.session.run("review.reply", &json!({"id":root,"text":"Reviewed"})).unwrap()["id"].as_u64().unwrap() as u32;
        let snapshot = Adapter(&mut app).capture().unwrap();
        let bytes = (snapshot.encode)().unwrap();
        Adapter(&mut app).open("Shared.wcraft.json", &bytes).unwrap();
        assert_eq!(app.session.doc.comments[&root].author, "Alice Subnet");
        assert_eq!(app.session.doc.comments[&reply].author, "Bob Subnet");
        assert_eq!(app.session.doc.comments[&reply].initials, "BS");
        assert_eq!(app.session.user_id.as_deref(), Some("account-b"));
    }

    #[test]
    fn account_profile_follows_host_and_cannot_restore_identity() {
        let mut app = wordcraft_ui_egui::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), Default::default());
        app.session.author = "Same display name".into();
        for id in [None, Some("account-a"), Some("account-a"), Some("account-b"), None] {
            Adapter(&mut app).set_user_profile(id.map(|id| sovereign_cloud_ui::UserProfile { id, display_name: "Subnet name" }));
            assert_eq!(app.session.user_id.as_deref(), id);
            if id.is_some() {
                assert_eq!(app.session.author, "Subnet name");
            }
        }
        Adapter(&mut app).set_user_profile(Some(sovereign_cloud_ui::UserProfile { id: "account-a", display_name: "Subnet name" }));
        let stored = serde_json::to_string(&app.prefs()).unwrap();
        assert!(!stored.contains("account-a"));
        let mut fresh = wordcraft_ui_egui::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), Default::default());
        crate::prefs::restore(&mut fresh, Some(&stored));
        assert!(fresh.session.user_id.is_none());
        assert_eq!(fresh.session.author, "Subnet name");
        app.session.set_document(wordcraft_doc::Document::new());
        assert_eq!(app.session.user_id.as_deref(), Some("account-a"));
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn workspace_creation_opens_one_native_document_and_preserves_it_on_repeat() {
        let mut app = wordcraft_ui_egui::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), Default::default());
        let mut adapter = Adapter(&mut app);
        adapter.create_document().unwrap();
        let first = adapter.document().unwrap();
        adapter.create_document().unwrap();
        assert_eq!(adapter.document().unwrap().id, first.id);
        assert!(!(adapter.capture().unwrap().encode)().unwrap().is_empty());
    }
}
