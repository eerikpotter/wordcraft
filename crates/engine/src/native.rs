//! Native JSON document persistence, including media omitted by the model's serde snapshot.
//! Older metadata-only JSON remains readable. No external files are opened while decoding.
use crate::cmd::insert::{base64_decode, base64_encode};
use std::{collections::BTreeMap, sync::Arc};
use wordcraft_doc::Document;

const MAX_NATIVE_BYTES: usize = 2 * 1024 * 1024 * 1024;

pub fn encode(document: &Document) -> Result<Vec<u8>, String> {
    encode_bounded(document, MAX_NATIVE_BYTES)
}

pub fn encode_bounded(document: &Document, max_bytes: usize) -> Result<Vec<u8>, String> {
    let media_bytes = document
        .media
        .values()
        .chain(document.passthrough.values())
        .try_fold(0usize, |sum, bytes| sum.checked_add(bytes.len()))
        .ok_or("Document is too large")?;
    if media_bytes > max_bytes / 4 * 3 {
        return Err("Native document exceeds the storage size limit".into());
    }
    let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("Invalid document")?;
    object.insert("wordcraftNativeVersion".into(), 1.into());
    for (field, parts) in [("embeddedMedia", &document.media), ("passthroughParts", &document.passthrough)] {
        let encoded: BTreeMap<_, _> = parts.iter().map(|(key, bytes)| (key, base64_encode(bytes))).collect();
        object.insert(field.into(), serde_json::to_value(encoded).map_err(|e| e.to_string())?);
    }
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if bytes.len() > max_bytes {
        return Err("Native document exceeds the storage size limit".into());
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Document, String> {
    decode_bounded(bytes, MAX_NATIVE_BYTES)
}

pub fn decode_bounded(bytes: &[u8], max_bytes: usize) -> Result<Document, String> {
    if bytes.len() > max_bytes {
        return Err("Native document exceeds the storage size limit".into());
    }
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("Invalid document")?;
    if object.get("wordcraftNativeVersion").is_some_and(|v| v.as_u64() != Some(1)) {
        return Err("Unsupported native document version".into());
    }
    let media = decode_parts(object.remove("embeddedMedia"))?;
    let passthrough = decode_parts(object.remove("passthroughParts"))?;
    let mut document: Document = serde_json::from_value(value).map_err(|e| e.to_string())?;
    document.media = media;
    document.passthrough = passthrough;
    document.ensure_nonempty();
    Ok(document)
}

fn decode_parts(value: Option<serde_json::Value>) -> Result<BTreeMap<String, Arc<Vec<u8>>>, String> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let parts: BTreeMap<String, String> = serde_json::from_value(value).map_err(|e| e.to_string())?;
    parts
        .into_iter()
        .map(|(name, encoded)| {
            let bytes = base64_decode(&encoded).ok_or("Invalid embedded file")?;
            if base64_encode(&bytes) != encoded {
                return Err("Invalid embedded file encoding".into());
            }
            Ok((name, Arc::new(bytes)))
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn native_document_round_trip_preserves_text_media_and_source_parts() {
        let mut session = crate::Session::new(Document::from_text("A document with embedded media"));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2).write_to(&mut png, image::ImageFormat::Png).unwrap();
        session.run("insert.picture", &serde_json::json!({"data": base64_encode(png.get_ref())})).unwrap();
        let mut document = session.doc;
        assert!(!document.media.is_empty());
        document.core.title = "Cloud test".into();
        document.passthrough.insert("customXml/item1.xml".into(), Arc::new(b"<custom>Original</custom>".to_vec()));
        let bytes = crate::io::save_bytes("test.wcraft.json", &document).unwrap();
        let restored = crate::io::open_bytes("test.wcraft.json", &bytes).unwrap();
        assert_eq!(restored, document);
    }
    #[test]
    fn old_json_stays_readable_and_invalid_media_or_versions_fail() {
        let document = Document::from_text("Legacy JSON");
        assert_eq!(decode(&serde_json::to_vec(&document).unwrap()).unwrap(), document);
        let mut value: serde_json::Value = serde_json::from_slice(&encode(&document).unwrap()).unwrap();
        value["wordcraftNativeVersion"] = 2.into();
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
        value["wordcraftNativeVersion"] = 1.into();
        value["embeddedMedia"] = serde_json::json!({"bad.png": "a"});
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
