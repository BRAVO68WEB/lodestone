use std::fs::File;
use std::io::copy;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Context};
use serde::Serialize;
use walkdir::WalkDir;

use crate::error::{Error, ErrorKind};
use crate::instance_import::{copy_tree, safe_unzip_to};
use crate::traits::t_server::State;

const BACKUPS_DIR: &str = "backups";

/// One zip stored in an instance's `backups` folder.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceBackup {
    pub name: String,
    pub bytes: u64,
    pub modified_ms: i64,
}

/// A zip written aside until the caller confirms the instance is still stopped.
pub struct PartialBackup {
    partial: PathBuf,
    dest: PathBuf,
    name: String,
}

impl Drop for PartialBackup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.partial);
    }
}

impl PartialBackup {
    pub fn finish(self) -> Result<InstanceBackup, Error> {
        std::fs::rename(&self.partial, &self.dest).context("Failed to store the backup")?;
        backup_info(&self.dest, self.name.clone())
    }

    pub fn discard(self) {}
}

/// Files have been swapped. Dropping this commits the restore by deleting the stash.
pub struct PreparedRestore {
    instance: PathBuf,
    stash: tempfile::TempDir,
    placed: Vec<std::ffi::OsString>,
}

impl PreparedRestore {
    pub fn commit(self) {}

    pub fn rollback(self) -> Result<(), Error> {
        for name in &self.placed {
            let path = self.instance.join(name);
            if path.symlink_metadata().is_ok() {
                remove_path(&path)?;
            }
        }
        for entry in std::fs::read_dir(self.stash.path()).context("Failed to read the restore stash")?
        {
            let entry = entry.context("Failed to read the restore stash")?;
            move_entry(&entry.path(), &self.instance.join(entry.file_name()))?;
        }
        Ok(())
    }
}

pub fn require_stopped(state: State, action: &str) -> Result<(), Error> {
    if state == State::Stopped {
        Ok(())
    } else {
        Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Stop the server before {action}"),
        })
    }
}

pub fn list_instance_backups(instance: &Path) -> Result<Vec<InstanceBackup>, Error> {
    let backups = instance.join(BACKUPS_DIR);
    if !backups.exists() {
        return Ok(Vec::new());
    }
    let mut listed = Vec::new();
    for entry in std::fs::read_dir(&backups).context("Failed to read backups")? {
        let entry = entry.context("Failed to read backups")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !valid_backup_name(&name) {
            continue;
        }
        let meta = entry.metadata().context("Failed to read a backup")?;
        if !meta.is_file() {
            continue;
        }
        listed.push(InstanceBackup {
            name,
            bytes: meta.len(),
            modified_ms: modified_ms(&meta),
        });
    }
    listed.sort_by(|left, right| {
        right
            .modified_ms
            .cmp(&left.modified_ms)
            .then_with(|| right.name.cmp(&left.name))
    });
    Ok(listed)
}

pub fn resolve_backup(instance: &Path, name: &str) -> Result<PathBuf, Error> {
    if !valid_backup_name(name) {
        return Err(bad_request("Backup name is not valid"));
    }
    let backups = instance.join(BACKUPS_DIR);
    let path = backups.join(name);
    if path.file_name().and_then(|part| part.to_str()) != Some(name) {
        return Err(bad_request("Backup name is not valid"));
    }
    let canon_backups = backups.canonicalize().map_err(|_| not_found("Backup not found"))?;
    let canon = path.canonicalize().map_err(|_| not_found("Backup not found"))?;
    if !canon.starts_with(&canon_backups) || !canon.is_file() {
        return Err(not_found("Backup not found"));
    }
    Ok(canon)
}

pub fn delete_instance_backup(instance: &Path, name: &str) -> Result<(), Error> {
    let path = resolve_backup(instance, name)?;
    std::fs::remove_file(path).context("Failed to delete the backup")?;
    Ok(())
}

pub fn begin_instance_backup(instance: &Path, now: DateTime<Utc>) -> Result<PartialBackup, Error> {
    let backups = instance.join(BACKUPS_DIR);
    std::fs::create_dir_all(&backups).context("Failed to create the backups folder")?;
    let name = unique_backup_name(&backups, now);
    let partial = backups.join(format!(".partial-{}", uuid::Uuid::new_v4()));
    if let Err(err) = write_instance_zip(instance, &partial) {
        let _ = std::fs::remove_file(&partial);
        return Err(err);
    }
    Ok(PartialBackup {
        partial,
        dest: backups.join(&name),
        name,
    })
}

pub fn prepare_restore(instance: &Path, archive: &Path) -> Result<PreparedRestore, Error> {
    let parent = instance
        .parent()
        .ok_or_else(|| bad_request("Instance path has no parent"))?;
    let staging = tempfile::tempdir_in(parent).context("Failed to stage the backup")?;
    safe_unzip_to(archive, staging.path())?;
    if !looks_like_instance(staging.path()) {
        return Err(bad_request("This backup does not contain an instance"));
    }
    let identity = std::fs::read(instance.join(".lodestone_config")).ok();
    let stash = tempfile::tempdir_in(parent).context("Failed to stash the current instance")?;
    let mut placed = Vec::new();
    let swapped = (|| -> Result<(), Error> {
        for entry in std::fs::read_dir(instance).context("Failed to read the instance")? {
            let entry = entry.context("Failed to read the instance")?;
            if entry.file_name() == BACKUPS_DIR {
                continue;
            }
            move_entry(&entry.path(), &stash.path().join(entry.file_name()))?;
        }
        for entry in std::fs::read_dir(staging.path()).context("Failed to read the backup")? {
            let entry = entry.context("Failed to read the backup")?;
            if entry.file_name() == BACKUPS_DIR {
                continue;
            }
            move_entry(&entry.path(), &instance.join(entry.file_name()))?;
            placed.push(entry.file_name());
        }
        if let Some(bytes) = &identity {
            std::fs::write(instance.join(".lodestone_config"), bytes)
                .context("Failed to keep the instance id")?;
        }
        Ok(())
    })();
    if let Err(err) = swapped {
        let prepared = PreparedRestore {
            instance: instance.to_path_buf(),
            stash,
            placed,
        };
        if let Err(rollback_err) = prepared.rollback() {
            return Err(Error {
                kind: ErrorKind::Internal,
                source: eyre!("Restore failed ({err}) and could not be undone ({rollback_err})"),
            });
        }
        return Err(err);
    }
    Ok(PreparedRestore {
        instance: instance.to_path_buf(),
        stash,
        placed,
    })
}

fn looks_like_instance(root: &Path) -> bool {
    root.join("server.properties").is_file()
        || root.join(".lodestone_minecraft_config.json").is_file()
        || root.join(".lodestone_config").is_file()
        || root.join("run.ts").is_file()
}

fn valid_backup_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".zip") else {
        return false;
    };
    (1..=80).contains(&stem.len())
        && !stem.starts_with('.')
        && stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn unique_backup_name(backups: &Path, now: DateTime<Utc>) -> String {
    let base = now.format("%Y%m%dT%H%M%SZ");
    let mut name = format!("{base}.zip");
    let mut suffix = 2u32;
    while backups.join(&name).exists() && suffix < 1000 {
        name = format!("{base}-{suffix}.zip");
        suffix += 1;
    }
    name
}

fn write_instance_zip(instance: &Path, dest: &Path) -> Result<(), Error> {
    let file = File::create(dest).context("Failed to create the backup")?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default().unix_permissions(0o644);
    let mut walker = WalkDir::new(instance).follow_links(false).into_iter();
    while let Some(item) = walker.next() {
        let entry = item.context("Failed to read the instance")?;
        let relative = entry
            .path()
            .strip_prefix(instance)
            .context("Failed to build a backup path")?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(bad_request(format!(
                "Backup refused a symlink at {}. Remove it and try again",
                relative.display()
            )));
        }
        if relative.components().next() == Some(Component::Normal(std::ffi::OsStr::new(BACKUPS_DIR)))
        {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        let name = zip_name(relative);
        if name.is_empty() {
            continue;
        }
        if entry.file_type().is_dir() {
            writer
                .add_directory(&name, options)
                .context("Failed to add a directory to the backup")?;
            continue;
        }
        if entry.file_type().is_file() {
            writer
                .start_file(&name, options)
                .context("Failed to add a file to the backup")?;
            let mut reader = File::open(entry.path()).context("Failed to read an instance file")?;
            copy(&mut reader, &mut writer).context("Failed to write an instance file")?;
        }
    }
    writer.finish().context("Failed to finish the backup")?;
    Ok(())
}

fn zip_name(relative: &Path) -> String {
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn move_entry(from: &Path, to: &Path) -> Result<(), Error> {
    if let Err(err) = std::fs::rename(from, to) {
        if err.raw_os_error() != Some(18) {
            return Err(Error {
                kind: ErrorKind::Internal,
                source: eyre!("Failed to move {}: {err}", from.display()),
            });
        }
        if from.is_dir() {
            copy_tree(from, to)?;
            std::fs::remove_dir_all(from).context("Failed to remove the original after copy")?;
        } else {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).context("Failed to create a directory")?;
            }
            std::fs::copy(from, to).context("Failed to copy a file")?;
            std::fs::remove_file(from).context("Failed to remove the original after copy")?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), Error> {
    let meta = std::fs::symlink_metadata(path).context("Failed to inspect a restored file")?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path).context("Failed to undo a restored directory")?;
    } else {
        std::fs::remove_file(path).context("Failed to undo a restored file")?;
    }
    Ok(())
}

fn backup_info(path: &Path, name: String) -> Result<InstanceBackup, Error> {
    let meta = std::fs::metadata(path).context("Failed to read the backup")?;
    Ok(InstanceBackup {
        name,
        bytes: meta.len(),
        modified_ms: modified_ms(&meta),
    })
}

fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn bad_request(message: impl std::fmt::Display) -> Error {
    Error {
        kind: ErrorKind::BadRequest,
        source: eyre!("{message}"),
    }
}

fn not_found(message: impl std::fmt::Display) -> Error {
    Error {
        kind: ErrorKind::NotFound,
        source: eyre!("{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        begin_instance_backup, delete_instance_backup, list_instance_backups, prepare_restore,
        require_stopped, resolve_backup, valid_backup_name,
    };
    use crate::traits::t_server::State;
    use chrono::{TimeZone, Utc};
    use std::io::Write;

    fn at(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> chrono::DateTime<Utc> {
        Utc.ymd(year, month, day).and_hms(hour, min, sec)
    }

    fn sample_instance(root: &std::path::Path) -> std::path::PathBuf {
        let instance = root.join("instance");
        std::fs::create_dir_all(instance.join("world")).unwrap();
        std::fs::create_dir_all(instance.join("backups")).unwrap();
        std::fs::write(instance.join("world/level.dat"), b"alpha").unwrap();
        std::fs::write(instance.join("notes.txt"), b"alpha").unwrap();
        std::fs::write(instance.join(".lodestone_config"), b"identity-a").unwrap();
        std::fs::write(instance.join("server.properties"), b"server-port=25565\n").unwrap();
        std::fs::write(instance.join("backups/keep.zip"), b"keep").unwrap();
        instance
    }

    fn zip_names(path: &std::path::Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn a_backup_leaves_out_the_backups_directory() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        let partial = begin_instance_backup(&instance, at(2026, 11, 4, 3, 15, 0)).unwrap();
        let saved = partial.finish().unwrap();
        assert_eq!(saved.name, "20261104T031500Z.zip");
        let names = zip_names(&instance.join("backups").join(&saved.name));
        assert!(names.iter().any(|name| name.contains("level.dat")), "{names:?}");
        assert!(names.iter().any(|name| name.contains("notes.txt")), "{names:?}");
        assert!(
            names.iter().all(|name| !name.contains("keep.zip") && !name.contains("backups")),
            "{names:?}"
        );
        let listed = list_instance_backups(&instance).unwrap();
        assert!(listed.iter().any(|item| item.name == saved.name));
        assert!(listed.iter().any(|item| item.name == "keep.zip"));
    }

    #[test]
    fn restore_replaces_the_world_and_keeps_backups_and_the_instance_id() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        let saved = begin_instance_backup(&instance, at(2026, 11, 4, 3, 15, 0))
            .unwrap()
            .finish()
            .unwrap();
        std::fs::write(instance.join("world/level.dat"), b"beta").unwrap();
        std::fs::remove_file(instance.join("notes.txt")).unwrap();
        std::fs::write(instance.join("extra.txt"), b"new").unwrap();

        let archive = resolve_backup(&instance, &saved.name).unwrap();
        prepare_restore(&instance, &archive).unwrap().commit();

        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"alpha");
        assert_eq!(std::fs::read(instance.join("notes.txt")).unwrap(), b"alpha");
        assert!(!instance.join("extra.txt").exists());
        assert_eq!(
            std::fs::read(instance.join(".lodestone_config")).unwrap(),
            b"identity-a"
        );
        assert_eq!(std::fs::read(instance.join("backups/keep.zip")).unwrap(), b"keep");
        assert!(instance.join("backups").join(&saved.name).is_file());
    }

    #[test]
    fn a_crafted_backup_cannot_replace_the_backups_folder_or_the_instance_id() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        let zip_path = instance.join("backups/crafted.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::FileOptions::default();
        zip.start_file("world/level.dat", options).unwrap();
        zip.write_all(b"new").unwrap();
        zip.start_file("server.properties", options).unwrap();
        zip.write_all(b"server-port=25570\n").unwrap();
        zip.start_file(".lodestone_config", options).unwrap();
        zip.write_all(b"identity-b").unwrap();
        zip.start_file("backups/evil.txt", options).unwrap();
        zip.write_all(b"evil").unwrap();
        zip.finish().unwrap();

        prepare_restore(&instance, &zip_path).unwrap().commit();
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"new");
        assert_eq!(
            std::fs::read_to_string(instance.join("server.properties")).unwrap(),
            "server-port=25570\n"
        );
        assert_eq!(
            std::fs::read(instance.join(".lodestone_config")).unwrap(),
            b"identity-a"
        );
        assert_eq!(std::fs::read(instance.join("backups/keep.zip")).unwrap(), b"keep");
        assert!(!instance.join("backups/evil.txt").exists());
    }

    #[test]
    fn an_empty_archive_does_not_replace_the_world() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        let zip_path = instance.join("backups/empty.zip");
        zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap())
            .finish()
            .unwrap();
        assert!(prepare_restore(&instance, &zip_path).is_err());
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"alpha");
    }

    #[test]
    fn a_zip_that_climbs_out_does_not_change_the_instance() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        let zip_path = instance.join("backups/slip.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::FileOptions::default();
        zip.start_file("../evil.txt", options).unwrap();
        zip.write_all(b"nope").unwrap();
        zip.finish().unwrap();
        assert!(prepare_restore(&instance, &zip_path).is_err());
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"alpha");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn a_backup_name_cannot_climb_out() {
        assert!(!valid_backup_name("../secret.zip"));
        assert!(!valid_backup_name("/tmp/secret.zip"));
        assert!(!valid_backup_name("nested/secret.zip"));
        assert!(!valid_backup_name(".zip"));
        assert!(valid_backup_name("20261104T031500Z.zip"));
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        std::fs::write(dir.path().join("secret.zip"), b"secret").unwrap();
        assert!(resolve_backup(&instance, "../secret.zip").is_err());
        assert!(resolve_backup(&instance, "keep.zip").is_ok());
        delete_instance_backup(&instance, "keep.zip").unwrap();
        assert!(resolve_backup(&instance, "keep.zip").is_err());
    }

    #[test]
    fn only_a_stopped_instance_can_be_backed_up() {
        assert!(require_stopped(State::Stopped, "taking a backup").is_ok());
        for state in [State::Starting, State::Running, State::Stopping, State::Error] {
            let err = require_stopped(state, "taking a backup").unwrap_err();
            assert!(err.to_string().contains("Stop the server"), "{err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_archived() {
        let dir = tempfile::tempdir().unwrap();
        let instance = sample_instance(dir.path());
        std::os::unix::fs::symlink(
            instance.join("world/level.dat"),
            instance.join("link.dat"),
        )
        .unwrap();
        let err = match begin_instance_backup(&instance, at(2026, 11, 4, 3, 15, 0)) {
            Ok(partial) => {
                drop(partial);
                panic!("symlink was archived");
            }
            Err(err) => err,
        };
        assert!(err.to_string().contains("symlink"), "{err}");
        for entry in std::fs::read_dir(instance.join("backups")).unwrap() {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            assert!(!name.ends_with(".zip") || name == "keep.zip", "{name}");
        }
    }
}
