use std::path::Path;

use color_eyre::eyre::eyre;

use crate::error::{Error, ErrorKind};

/// A sidebar group name. Empty means the instance stays ungrouped.
pub fn normalize_instance_folder(raw: &str) -> Result<String, Error> {
    let folder = raw.trim();
    if folder.is_empty() {
        return Ok(String::new());
    }
    if folder.chars().count() > 32 {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Folder name cannot be longer than 32 characters"),
        });
    }
    if folder == "."
        || folder == ".."
        || folder.chars().any(|ch| ch == '/' || ch == '\\' || ch.is_control())
    {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Folder name cannot include a path"),
        });
    }
    Ok(folder.to_string())
}

pub async fn read_instance_folder(instance_path: &Path) -> String {
    match tokio::fs::read_to_string(instance_path.join(".lodestone_folder")).await {
        Ok(text) => normalize_instance_folder(&text).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

pub async fn write_instance_folder(instance_path: &Path, raw: &str) -> Result<(), Error> {
    let folder = normalize_instance_folder(raw)?;
    let path = instance_path.join(".lodestone_folder");
    if folder.is_empty() {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(Error {
                kind: ErrorKind::Internal,
                source: err.into(),
            }),
        }
    } else {
        crate::util::write_atomic(&path, folder.as_bytes()).await
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_instance_folder;

    #[test]
    fn folder_names_are_trimmed_and_paths_are_rejected() {
        assert_eq!(normalize_instance_folder("  Survival  ").unwrap(), "Survival");
        assert_eq!(normalize_instance_folder("   ").unwrap(), "");
        assert!(normalize_instance_folder("a/b").is_err());
        assert!(normalize_instance_folder(r"a\b").is_err());
        assert!(normalize_instance_folder("..").is_err());
        assert!(normalize_instance_folder(&"x".repeat(33)).is_err());
    }

    #[tokio::test]
    async fn a_folder_is_written_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        super::write_instance_folder(dir.path(), " Survival ").await.unwrap();
        assert_eq!(super::read_instance_folder(dir.path()).await, "Survival");
        super::write_instance_folder(dir.path(), " ").await.unwrap();
        assert_eq!(super::read_instance_folder(dir.path()).await, "");
        assert!(!dir.path().join(".lodestone_folder").exists());
    }
}
