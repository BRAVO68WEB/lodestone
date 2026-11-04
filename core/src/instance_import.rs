use std::fs::File;
use std::io::copy;
use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{eyre, Context};

use crate::error::{Error, ErrorKind};
use crate::implementations::minecraft::{Flavour, RestoreConfig};
use crate::traits::t_configurable::GameType;
use crate::types::{DotLodestoneConfig, InstanceUuid};

/// Keep only normal path pieces. `..`, absolute paths, and Windows prefixes are rejected.
pub fn safe_zip_relative(name: &str) -> Result<PathBuf, Error> {
    let path = Path::new(name);
    if path.is_absolute() || name.contains(':') {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Zip entry is absolute: {name}"),
        });
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => {
                return Err(Error {
                    kind: ErrorKind::BadRequest,
                    source: eyre!("Zip entry escapes the instance directory: {name}"),
                });
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Zip entry has no file name"),
        });
    }
    Ok(relative)
}

pub fn safe_unzip_to(archive_path: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest).context("Failed to create import directory")?;
    let file = File::open(archive_path).context("Failed to open import zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read import zip")?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .context("Failed to read a zip entry")?;
        let relative = safe_zip_relative(entry.name())?;
        let target = dest.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).context("Failed to create a directory from the zip")?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).context("Failed to create a directory from the zip")?;
        }
        let mut out = File::create(&target).context("Failed to create an imported file")?;
        copy(&mut entry, &mut out).context("Failed to write an imported file")?;
    }
    Ok(())
}

/// Many server zips wrap the world in one folder. Lift that folder up when it holds `server.properties`.
pub fn hoist_single_server_dir(dest: &Path) -> Result<(), Error> {
    if dest.join("server.properties").is_file() {
        return Ok(());
    }
    let mut wrappers = Vec::new();
    for entry in std::fs::read_dir(dest).context("Failed to read import directory")? {
        let entry = entry.context("Failed to read import directory")?;
        if entry.path().is_dir() {
            wrappers.push(entry.path());
        }
    }
    if wrappers.len() == 1 && wrappers[0].join("server.properties").is_file() {
        let wrapper = wrappers.remove(0);
        for child in std::fs::read_dir(&wrapper).context("Failed to read wrapped server")? {
            let child = child.context("Failed to read wrapped server")?;
            let target = dest.join(child.file_name());
            std::fs::rename(child.path(), &target).context("Failed to lift wrapped server")?;
        }
        std::fs::remove_dir(&wrapper).context("Failed to remove the wrapper directory")?;
    }
    Ok(())
}

pub fn server_port_from_properties(text: &str) -> Option<u32> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("server-port=") {
            return value.trim().parse::<u32>().ok().filter(|port| *port > 0);
        }
    }
    None
}

pub fn import_directory_name(name: &str, uuid_prefix: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(32)
        .collect();
    let cleaned = if cleaned.is_empty() {
        "imported".to_string()
    } else {
        cleaned
    };
    format!("{cleaned}-{uuid_prefix}")
}

pub fn path_is_inside(root: &Path, source: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(source) = source.canonicalize() else {
        return false;
    };
    source.starts_with(root)
}

pub fn copy_tree(source: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest).context("Failed to create import directory")?;
    for entry in std::fs::read_dir(source).context("Failed to read the source directory")? {
        let entry = entry.context("Failed to read the source directory")?;
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).context("Failed to copy an imported file")?;
        }
    }
    Ok(())
}

pub fn write_import_files(
    dest: &Path,
    name: &str,
    uuid: InstanceUuid,
) -> Result<DotLodestoneConfig, Error> {
    let properties = dest.join("server.properties");
    let port = std::fs::read_to_string(&properties)
        .ok()
        .and_then(|text| server_port_from_properties(&text))
        .unwrap_or(25565);
    if !properties.is_file() {
        std::fs::write(&properties, format!("server-port={port}\n"))
            .context("Failed to write server.properties")?;
    }
    let config = DotLodestoneConfig::new(uuid, GameType::MinecraftJava);
    std::fs::write(
        dest.join(".lodestone_config"),
        serde_json::to_vec_pretty(&config).context("Failed to serialize .lodestone_config")?,
    )
    .context("Failed to write .lodestone_config")?;
    let jar = crate::implementations::minecraft::util::read_minecraft_jar_info(
        &dest.join("server.jar"),
    );
    let version = jar
        .as_ref()
        .map(|info| info.version.clone())
        .unwrap_or_else(|| "imported".to_string());
    let jre_major_version = jar.as_ref().map(|info| info.java_major).unwrap_or(17);
    let restore = RestoreConfig {
        name: name.to_string(),
        version,
        flavour: Flavour::Vanilla,
        description: "Imported server".to_string(),
        cmd_args: Vec::new(),
        java_cmd: None,
        port,
        min_ram: 1024,
        max_ram: 2048,
        auto_start: false,
        restart_on_crash: false,
        backup_period: None,
        jre_major_version,
        has_started: true,
    };
    std::fs::write(
        dest.join(".lodestone_minecraft_config.json"),
        serde_json::to_vec_pretty(&restore).context("Failed to serialize minecraft config")?,
    )
    .context("Failed to write minecraft config")?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::{
        hoist_single_server_dir, import_directory_name, path_is_inside, safe_unzip_to,
        safe_zip_relative, server_port_from_properties, write_import_files,
    };
    use std::io::Write;

    use crate::types::InstanceUuid;

    #[test]
    fn a_zip_entry_that_climbs_out_is_rejected() {
        assert!(safe_zip_relative("../evil.txt").is_err());
        assert!(safe_zip_relative("/etc/passwd").is_err());
        assert_eq!(
            safe_zip_relative("world/level.dat").unwrap(),
            std::path::PathBuf::from("world/level.dat")
        );
    }

    #[test]
    fn a_zip_with_a_parent_path_does_not_escape() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("server.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::FileOptions::default();
        zip.start_file("../evil.txt", options).unwrap();
        zip.write_all(b"nope").unwrap();
        zip.finish().unwrap();

        let dest = dir.path().join("instance");
        assert!(safe_unzip_to(&zip_path, &dest).is_err());
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn a_server_zip_keeps_the_port_and_a_new_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("server.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::FileOptions::default();
        zip.start_file("server.properties", options).unwrap();
        zip.write_all(b"motd=hi\nserver-port=25570\n").unwrap();
        zip.finish().unwrap();

        let dest = dir.path().join("imported");
        safe_unzip_to(&zip_path, &dest).unwrap();
        hoist_single_server_dir(&dest).unwrap();
        let uuid = InstanceUuid::default();
        let config = write_import_files(&dest, "Skyblock", uuid.clone()).unwrap();
        assert_eq!(config.uuid(), &uuid);
        let on_disk: crate::types::DotLodestoneConfig = serde_json::from_slice(
            &std::fs::read(dest.join(".lodestone_config")).unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk.uuid(), &uuid);
        let properties = std::fs::read_to_string(dest.join("server.properties")).unwrap();
        assert_eq!(server_port_from_properties(&properties), Some(25570));
        assert_eq!(import_directory_name("Sky block!", "abcd1234"), "Skyblock-abcd1234");
    }

    #[test]
    fn a_source_outside_the_data_directory_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let inside = root.path().join("instances");
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(&inside).unwrap();
        assert!(path_is_inside(root.path(), &inside));
        assert!(!path_is_inside(root.path(), outside.path()));
    }

    #[test]
    fn java_for_server_import_stores_jar_version() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("imported");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("server.properties"), b"server-port=25565\n").unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dest.join("server.jar")).unwrap());
        zip.start_file("version.json", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(br#"{"id":"26.3","java_version":25}"#)
            .unwrap();
        zip.finish().unwrap();

        write_import_files(&dest, "mc-26.3", InstanceUuid::default()).unwrap();
        let restore: crate::implementations::minecraft::RestoreConfig = serde_json::from_slice(
            &std::fs::read(dest.join(".lodestone_minecraft_config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(restore.version, "26.3");
        assert_eq!(restore.jre_major_version, 25);
        assert!(restore.java_cmd.is_none());

        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("server.properties"), b"server-port=25565\n").unwrap();
        write_import_files(&plain, "plain", InstanceUuid::default()).unwrap();
        let plain_restore: crate::implementations::minecraft::RestoreConfig =
            serde_json::from_slice(
                &std::fs::read(plain.join(".lodestone_minecraft_config.json")).unwrap(),
            )
            .unwrap();
        assert_eq!(plain_restore.version, "imported");
        assert_eq!(plain_restore.jre_major_version, 17);
    }
}
