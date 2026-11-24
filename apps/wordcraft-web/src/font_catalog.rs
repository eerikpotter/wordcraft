//! Supply the shared font picker with the same in-memory fonts the browser renderer uses.
//! Browser catalogs cannot discover fonts by scanning native font folders.

use craft::font_manager::{Catalog, Source};
use wordcraft_fonts::FontDb;

#[derive(Default)]
pub(crate) struct BrowserFonts {
    generation: Option<u64>,
}

impl BrowserFonts {
    pub(crate) fn sync(&mut self, db: &FontDb, catalog: &Catalog) {
        let generation = db.generation();
        if self.generation == Some(generation) {
            return;
        }
        let faces: Vec<_> = db.families().iter().flat_map(|family| db.styles(family).into_iter().map(move |style| db.face(family, &style))).collect();
        let mut files = std::collections::HashSet::new();
        catalog.remove_added(Source::App);
        for face in &faces {
            // Variable faces and document aliases may share identical font-file bytes.
            if files.insert(face.data()) {
                if let Err(error) = catalog.add_bytes(face.data().to_vec(), Source::App, &Default::default()) {
                    log::warn!("Could not add browser font {} to the picker: {error}", face.family);
                }
            }
        }
        self.generation = Some(generation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_catalog_is_ready_and_refreshes_without_duplicate_font_files() {
        let db = FontDb::with_font_dirs(Vec::new());
        let catalog = Catalog::of_files(Vec::new());
        let mut fonts = BrowserFonts::default();
        assert!(!catalog.is_ready());
        fonts.sync(&db, &catalog);
        assert!(catalog.is_ready());
        assert!(catalog.families().iter().any(|family| family == "Source Serif 4"));
        let count = catalog.faces().len();
        let catalog_generation = catalog.generation();
        fonts.sync(&db, &catalog);
        assert_eq!(catalog.generation(), catalog_generation);
        let generation = db.generation();
        assert!(db.add_document_fonts(vec![(wordcraft_fonts::bundled()[0].to_vec(), "Document Font Alias".into())]) > 0);
        assert!(db.generation() > generation);
        fonts.sync(&db, &catalog);
        assert!(catalog.generation() > catalog_generation);
        assert_eq!(catalog.faces().len(), count);
    }
}
