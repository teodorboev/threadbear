// Windows' own previews read through the app's path guard, rather than asking
// the runner to resolve the path a second time. Same files.read/files.list
// answer shapes, so the pane and picture viewer share their normal rendering.
use crate::open_policy;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::io::{self, Read};
use std::path::Path;

fn modified(meta: &std::fs::Metadata) -> String {
    meta.modified().map(|at| chrono::DateTime::<chrono::Utc>::from(at).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_default()
}

pub fn read(params: &Value) -> io::Result<Value> {
    let path = params["path"].as_str().ok_or_else(|| io::Error::other("path must be a string"))?;
    let file = open_policy::local_open(Path::new(path)).map_err(|e| if e.kind() == io::ErrorKind::NotFound { io::Error::new(e.kind(), format!("No such file: {path}")) } else { e })?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other(format!("Not a file: {path}")));
    }
    let modified = modified(&meta);
    if params["known"]["size"].as_u64() == Some(meta.len()) && params["known"]["modifiedAt"].as_str() == Some(&modified) {
        return Ok(json!({ "path": path, "unchanged": true, "size": meta.len(), "modifiedAt": modified }));
    }
    let max = params["maxBytes"].as_u64().filter(|n| *n > 0).unwrap_or(16 * 1024 * 1024).min(48 * 1024 * 1024);
    let mut bytes = Vec::new();
    file.take(max).read_to_end(&mut bytes)?;
    Ok(json!({ "path": path, "size": meta.len(), "modifiedAt": modified, "data": STANDARD.encode(&bytes), "truncated": meta.len() > bytes.len() as u64 }))
}

pub fn list(params: &Value) -> io::Result<Value> {
    let path = params["path"].as_str().ok_or_else(|| io::Error::other("path must be a string"))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in open_policy::local_read_dir(Path::new(path)).map_err(|e| if e.kind() == io::ErrorKind::NotFound { io::Error::new(e.kind(), format!("No such folder: {path}")) } else { e })? {
        let entry = entry?;
        if entries.len() == 5000 {
            truncated = true;
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Never stat a link through the runner. An unsafe or broken link
        // stays visible by name, and trying to open it will be refused.
        let meta = open_policy::local_metadata(&Path::new(path).join(&name)).ok();
        let folder = meta.as_ref().is_some_and(|m| m.is_dir());
        let mut row = json!({ "name": name, "isFolder": folder, "hidden": name.starts_with('.') });
        if let Some(meta) = meta {
            row["modifiedAt"] = json!(modified(&meta));
            if !folder {
                row["size"] = json!(meta.len());
            }
        }
        entries.push(row);
    }
    Ok(json!({ "path": path, "entries": entries, "truncated": truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_previews_keep_the_protocols_read_and_list_shapes() {
        let dir = std::env::temp_dir().join(format!("tb-local-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, b"hello").unwrap();
        let read = read(&json!({ "path": file, "maxBytes": 3 })).unwrap();
        let decoded = crate::folders::FileRead::decode(&read).unwrap();
        assert_eq!(decoded.data, b"hel");
        assert!(decoded.truncated);
        let listing = crate::folders::FolderContents::decode(&list(&json!({ "path": dir })).unwrap());
        assert!(listing.entries.iter().any(|e| e.name == "notes.md" && !e.is_folder && e.size == Some(5)));
        assert!(super::read(&json!({ "path": dir })).unwrap_err().to_string().starts_with("Not a file"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
