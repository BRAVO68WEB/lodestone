use std::path::Path;

use color_eyre::eyre::eyre;
use color_eyre::eyre::Context;

use crate::error::{Error, ErrorKind};
use crate::instance_import::copy_tree;

pub struct LoaderUpdate {
    pub version: String,
    pub loader_version: Option<String>,
    pub installer_version: Option<String>,
    pub build: Option<String>,
}

const PROTECTED_DIRS: &[&str] = &[
    "world",
    "world_nether",
    "world_the_end",
    "mods",
    "backups",
];
const PROTECTED_FILES: &[&str] = &[
    "server.properties",
    ".lodestone_config",
    ".lodestone_minecraft_config.json",
    ".lodestone_folder",
];

pub fn require_token(value: &str, label: &str) -> Result<String, Error> {
    if value.is_empty()
        || value.len() > 64
        || value.contains("..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(bad_request(format!("{label} is not valid")));
    }
    Ok(value.to_string())
}

pub fn optional_token(value: Option<String>, label: &str) -> Result<Option<String>, Error> {
    match value {
        None => Ok(None),
        Some(text) if text.trim().is_empty() => Ok(None),
        Some(text) => Ok(Some(require_token(text.trim(), label)?)),
    }
}

/// Replace `server.jar` only. A failed rename puts the previous jar back.
pub fn replace_server_jar(instance: &Path, source: &Path) -> Result<(), Error> {
    if !source.is_file() {
        return Err(bad_request("The downloaded server jar is missing"));
    }
    let dest = instance.join("server.jar");
    let partial = instance.join(".server.jar.partial");
    let previous = instance.join(".server.jar.previous");
    std::fs::copy(source, &partial).context("Failed to write the new server jar")?;
    if previous.exists() {
        std::fs::remove_file(&previous).context("Failed to clear the previous server jar")?;
    }
    if dest.exists() {
        std::fs::rename(&dest, &previous).context("Failed to move the old server jar aside")?;
    }
    if let Err(err) = std::fs::rename(&partial, &dest) {
        if previous.exists() {
            let _ = std::fs::rename(&previous, &dest);
        }
        return Err(Error {
            kind: ErrorKind::Internal,
            source: eyre!("Failed to install the new server jar: {err}"),
        });
    }
    let _ = std::fs::remove_file(&previous);
    let _ = std::fs::remove_file(&partial);
    Ok(())
}

pub fn snapshot_protected(instance: &Path, stash: &Path) -> Result<(), Error> {
    for name in PROTECTED_DIRS {
        let source = instance.join(name);
        if source.is_dir() {
            copy_tree(&source, &stash.join(name))?;
        }
    }
    for name in PROTECTED_FILES {
        let source = instance.join(name);
        if source.is_file() {
            std::fs::copy(&source, stash.join(name))
                .context("Failed to snapshot an instance file")?;
        }
    }
    Ok(())
}

pub fn restore_protected(instance: &Path, stash: &Path) -> Result<(), Error> {
    for name in PROTECTED_DIRS {
        let saved = stash.join(name);
        let dest = instance.join(name);
        if !saved.is_dir() {
            continue;
        }
        if dest.exists() {
            std::fs::remove_dir_all(&dest).context("Failed to clear a protected directory")?;
        }
        copy_tree(&saved, &dest)?;
    }
    for name in PROTECTED_FILES {
        let saved = stash.join(name);
        if saved.is_file() {
            std::fs::copy(&saved, instance.join(name))
                .context("Failed to restore an instance file")?;
        }
    }
    Ok(())
}

fn bad_request(message: impl std::fmt::Display) -> Error {
    Error {
        kind: ErrorKind::BadRequest,
        source: eyre!("{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        replace_server_jar, require_token, restore_protected, snapshot_protected,
    };

    fn sample(root: &std::path::Path) -> std::path::PathBuf {
        let instance = root.join("instance");
        std::fs::create_dir_all(instance.join("world")).unwrap();
        std::fs::create_dir_all(instance.join("mods")).unwrap();
        std::fs::write(instance.join("world/level.dat"), b"world").unwrap();
        std::fs::write(instance.join("mods/example.jar"), b"mod").unwrap();
        std::fs::write(instance.join("server.properties"), b"server-port=25565\n").unwrap();
        std::fs::write(instance.join("server.jar"), b"old-jar").unwrap();
        std::fs::write(instance.join(".lodestone_config"), b"identity").unwrap();
        instance
    }

    #[test]
    fn replace_server_jar_leaves_the_world_properties_and_mods() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample(dir.path());
        let source = dir.path().join("new.jar");
        std::fs::write(&source, b"new-jar").unwrap();
        replace_server_jar(&instance, &source).unwrap();
        assert_eq!(std::fs::read(instance.join("server.jar")).unwrap(), b"new-jar");
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"world");
        assert_eq!(std::fs::read(instance.join("mods/example.jar")).unwrap(), b"mod");
        assert_eq!(
            std::fs::read(instance.join("server.properties")).unwrap(),
            b"server-port=25565\n"
        );
        assert_eq!(std::fs::read(instance.join(".lodestone_config")).unwrap(), b"identity");
        assert!(!instance.join(".server.jar.partial").exists());
        assert!(!instance.join(".server.jar.previous").exists());
    }

    #[test]
    fn a_loader_install_puts_back_world_properties_and_mods() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample(dir.path());
        let stash = dir.path().join("stash");
        std::fs::create_dir_all(&stash).unwrap();
        snapshot_protected(&instance, &stash).unwrap();

        std::fs::remove_dir_all(instance.join("world")).unwrap();
        std::fs::remove_file(instance.join("mods/example.jar")).unwrap();
        std::fs::write(instance.join("mods/injected.jar"), b"nope").unwrap();
        std::fs::write(instance.join("server.properties"), b"server-port=1\n").unwrap();
        std::fs::write(instance.join(".lodestone_config"), b"changed").unwrap();
        std::fs::create_dir_all(instance.join("libraries")).unwrap();
        std::fs::write(instance.join("libraries/loader.txt"), b"loader").unwrap();
        std::fs::write(instance.join("server.jar"), b"new-jar").unwrap();

        restore_protected(&instance, &stash).unwrap();
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"world");
        assert_eq!(std::fs::read(instance.join("mods/example.jar")).unwrap(), b"mod");
        assert!(!instance.join("mods/injected.jar").exists());
        assert_eq!(
            std::fs::read(instance.join("server.properties")).unwrap(),
            b"server-port=25565\n"
        );
        assert_eq!(std::fs::read(instance.join(".lodestone_config")).unwrap(), b"identity");
        assert_eq!(std::fs::read(instance.join("libraries/loader.txt")).unwrap(), b"loader");
        assert_eq!(std::fs::read(instance.join("server.jar")).unwrap(), b"new-jar");
    }

    #[test]
    fn a_version_token_cannot_escape() {
        assert_eq!(require_token("1.21.1", "Version").unwrap(), "1.21.1");
        assert_eq!(require_token("24w14a", "Version").unwrap(), "24w14a");
        assert_eq!(
            require_token("1.20.1-47.2.0", "Build").unwrap(),
            "1.20.1-47.2.0"
        );
        assert!(require_token("../server", "Version").is_err());
        assert!(require_token("1.21/../x", "Version").is_err());
        assert!(require_token("", "Version").is_err());
        assert!(require_token("has space", "Version").is_err());
    }
}
