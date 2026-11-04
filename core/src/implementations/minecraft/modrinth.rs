use std::collections::HashSet;
use std::path::{Component, Path};
use std::time::Duration;

use color_eyre::eyre::{eyre, Context};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind};

const MAX_JARS: usize = 16;
const MAX_JAR_BYTES: usize = 128 * 1024 * 1024;
const MODRINTH_API: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Serialize)]
pub struct ModProject {
    pub project_id: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModJar {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedMod {
    pub project_id: String,
    pub version: String,
    pub filename: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchHit {
    project_id: String,
    title: String,
    description: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectVersion {
    #[serde(rename = "id")]
    _id: String,
    project_id: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    environment: String,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    primary: bool,
    #[serde(default)]
    file_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Dependency {
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    dependency_type: String,
}

pub fn clean_search(query: &str) -> Result<String, Error> {
    let query = query.trim();
    if query.is_empty() || query.len() > 64 || query.chars().any(|ch| ch.is_control()) {
        return Err(bad_request("Search text is not valid"));
    }
    Ok(query.to_string())
}

pub fn valid_project_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub fn valid_game_version(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value.chars().any(|ch| ch.is_ascii_digit())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        && !value.contains("..")
}

pub fn safe_mod_filename(name: &str) -> Result<(), Error> {
    let path = Path::new(name);
    let one_piece = path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)));
    if !one_piece
        || name.len() > 128
        || !name.to_ascii_lowercase().ends_with(".jar")
        || name.contains("..")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+'))
    {
        return Err(bad_request("Mod file name is not valid"));
    }
    Ok(())
}

pub fn allowed_download_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.port().is_none()
        && parsed.host_str() == Some("cdn.modrinth.com")
}

pub fn list_mod_jars(instance: &Path) -> Result<Vec<ModJar>, Error> {
    let mods = instance.join("mods");
    if !mods.exists() {
        return Ok(Vec::new());
    }
    let mut listed = Vec::new();
    for entry in std::fs::read_dir(&mods).context("Failed to read mods")? {
        let entry = entry.context("Failed to read mods")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if safe_mod_filename(&name).is_err() {
            continue;
        }
        let meta = entry.metadata().context("Failed to read a mod")?;
        if !meta.is_file() {
            continue;
        }
        listed.push(ModJar {
            name,
            bytes: meta.len(),
        });
    }
    listed.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(listed)
}

pub fn write_mod_jar(instance: &Path, filename: &str, bytes: &[u8]) -> Result<(), Error> {
    safe_mod_filename(filename)?;
    if bytes.len() < 4 || &bytes[..4] != b"PK\x03\x04" {
        return Err(bad_request("Downloaded file is not a jar"));
    }
    if bytes.len() > MAX_JAR_BYTES {
        return Err(bad_request("Mod jar is too large"));
    }
    let mods = instance.join("mods");
    std::fs::create_dir_all(&mods).context("Failed to create the mods folder")?;
    let canon_instance = instance
        .canonicalize()
        .context("Failed to read the instance")?;
    let canon_mods = mods.canonicalize().context("Failed to read the mods folder")?;
    if !canon_mods.starts_with(&canon_instance) {
        return Err(bad_request("The mods folder is outside the instance"));
    }
    let dest = canon_mods.join(filename);
    let partial = canon_mods.join(format!(".{filename}.partial"));
    let previous = canon_mods.join(format!(".{filename}.previous"));
    std::fs::write(&partial, bytes).context("Failed to write the mod")?;
    if previous.exists() {
        std::fs::remove_file(&previous).context("Failed to clear a previous mod")?;
    }
    if dest.exists() {
        std::fs::rename(&dest, &previous).context("Failed to move the old mod aside")?;
    }
    if let Err(err) = std::fs::rename(&partial, &dest) {
        let _ = std::fs::remove_file(&partial);
        if previous.exists() {
            let _ = std::fs::rename(&previous, &dest);
        }
        return Err(Error {
            kind: ErrorKind::Internal,
            source: eyre!("Failed to install {filename}: {err}"),
        });
    }
    let _ = std::fs::remove_file(&previous);
    Ok(())
}

pub fn remove_mod_jar(instance: &Path, filename: &str) -> Result<(), Error> {
    safe_mod_filename(filename)?;
    let mods = instance.join("mods");
    let canon_mods = mods.canonicalize().map_err(|_| not_found("Mod not found"))?;
    let canon_instance = instance
        .canonicalize()
        .map_err(|_| not_found("Mod not found"))?;
    if !canon_mods.starts_with(&canon_instance) {
        return Err(bad_request("The mods folder is outside the instance"));
    }
    let path = canon_mods.join(filename);
    let canon = path.canonicalize().map_err(|_| not_found("Mod not found"))?;
    if !canon.starts_with(&canon_mods) || !canon.is_file() {
        return Err(not_found("Mod not found"));
    }
    std::fs::remove_file(&canon).context("Failed to delete the mod")?;
    Ok(())
}

struct Selection {
    planned: PlannedMod,
    project_id: String,
    deps: Vec<(String, Option<String>)>,
}

fn select_one(versions: &[ProjectVersion], loader: &str, game: &str) -> Result<Selection, Error> {
    let chosen = choose_version(versions, loader, game)
        .ok_or_else(|| bad_request(format!("No server build for {loader} {game}")))?;
    let deps = chosen
        .dependencies
        .iter()
        .filter(|dep| dep.dependency_type == "required")
        .map(|dep| {
            dep.project_id
                .clone()
                .map(|project_id| (project_id, dep.version_id.clone()))
                .ok_or_else(|| bad_request("A required dependency is not on Modrinth"))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let file = jar_file(chosen)?;
    if !allowed_download_url(&file.url) {
        return Err(bad_request("Mod download host is not allowed"));
    }
    Ok(Selection {
        project_id: chosen.project_id.clone(),
        planned: PlannedMod {
            project_id: chosen.project_id.clone(),
            version: chosen.version_number.clone(),
            filename: file.filename.clone(),
            url: file.url.clone(),
        },
        deps,
    })
}

fn plan_install(
    root_versions: &[ProjectVersion],
    loader: &str,
    game: &str,
    lookup: &mut dyn FnMut(&str, Option<&str>) -> Result<Vec<ProjectVersion>, Error>,
) -> Result<Vec<PlannedMod>, Error> {
    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    walk(root_versions, loader, game, lookup, &mut planned, &mut seen)?;
    unique_names(&planned)
}

fn walk(
    versions: &[ProjectVersion],
    loader: &str,
    game: &str,
    lookup: &mut dyn FnMut(&str, Option<&str>) -> Result<Vec<ProjectVersion>, Error>,
    planned: &mut Vec<PlannedMod>,
    seen: &mut HashSet<String>,
) -> Result<(), Error> {
    let selection = select_one(versions, loader, game)?;
    if !seen.insert(selection.project_id) {
        return Ok(());
    }
    for (project_id, version_id) in selection.deps {
        if seen.contains(&project_id) {
            continue;
        }
        let next = lookup(&project_id, version_id.as_deref())?;
        walk(&next, loader, game, lookup, planned, seen)?;
    }
    if planned.len() >= MAX_JARS {
        return Err(bad_request("This mod needs too many required dependencies"));
    }
    planned.push(selection.planned);
    Ok(())
}

fn unique_names(planned: &[PlannedMod]) -> Result<Vec<PlannedMod>, Error> {
    let mut names = HashSet::new();
    for item in planned {
        if !names.insert(item.filename.clone()) {
            return Err(bad_request(format!(
                "Two mods want the same file name {}",
                item.filename
            )));
        }
    }
    Ok(planned.to_vec())
}

fn choose_version<'a>(
    versions: &'a [ProjectVersion],
    loader: &str,
    game: &str,
) -> Option<&'a ProjectVersion> {
    versions
        .iter()
        .filter(|version| version_fits(version, loader, game))
        .min_by(|left, right| {
            channel_rank(&left.version_type)
                .cmp(&channel_rank(&right.version_type))
                .then_with(|| right.date_published.cmp(&left.date_published))
        })
}

fn version_fits(version: &ProjectVersion, loader: &str, game: &str) -> bool {
    runs_on_server(&version.environment)
        && version.loaders.iter().any(|item| item == loader)
        && version.game_versions.iter().any(|item| item == game)
}

fn runs_on_server(environment: &str) -> bool {
    !matches!(environment, "client_only" | "singleplayer_only")
}

fn channel_rank(channel: &str) -> u8 {
    match channel {
        "release" => 0,
        "beta" => 1,
        "alpha" => 2,
        _ => 3,
    }
}

fn jar_file(version: &ProjectVersion) -> Result<&VersionFile, Error> {
    let file = version
        .files
        .iter()
        .find(|file| file.primary && installable_jar(file))
        .or_else(|| version.files.iter().find(|file| installable_jar(file)))
        .ok_or_else(|| bad_request("This version has no jar"))?;
    safe_mod_filename(&file.filename)?;
    Ok(file)
}

fn installable_jar(file: &VersionFile) -> bool {
    let extra = matches!(
        file.file_type.as_deref(),
        Some(
            "sources-jar"
                | "dev-jar"
                | "javadoc-jar"
                | "signature"
                | "required-resource-pack"
                | "optional-resource-pack"
        )
    );
    file.filename.to_ascii_lowercase().ends_with(".jar") && !extra
}

pub async fn search_projects(
    query: &str,
    loader: &str,
    game: &str,
) -> Result<Vec<ModProject>, Error> {
    let query = clean_search(query)?;
    let facets = serde_json::json!([
        ["project_type:mod"],
        [format!("categories:{loader}")],
        [format!("versions:{game}")],
        [
            "environment:client_and_server",
            "environment:client_only_server_optional",
            "environment:server_only",
            "environment:server_only_client_optional",
            "environment:dedicated_server_only",
            "environment:client_or_server",
            "environment:client_or_server_prefers_both",
            "environment:unknown"
        ]
    ]);
    let response: SearchResponse = modrinth_get(
        &format!("{MODRINTH_API}/search"),
        &[
            ("query", query),
            ("limit", "8".to_string()),
            ("facets", facets.to_string()),
        ],
    )
    .await?;
    Ok(response
        .hits
        .into_iter()
        .map(|hit| ModProject {
            project_id: hit.project_id,
            title: hit.title,
            description: clip(&hit.description, 240),
        })
        .collect())
}

pub async fn plan_project(
    project_id: &str,
    loader: &str,
    game: &str,
) -> Result<Vec<PlannedMod>, Error> {
    if !valid_project_id(project_id) {
        return Err(bad_request("Project id is not valid"));
    }
    let root = project_versions(project_id, loader, game).await?;
    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    enum Frame {
        Enter(Vec<ProjectVersion>),
        Finish(PlannedMod),
    }
    let mut stack = vec![Frame::Enter(root)];
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Finish(item) => {
                if planned.len() >= MAX_JARS {
                    return Err(bad_request(
                        "This mod needs too many required dependencies",
                    ));
                }
                planned.push(item);
            }
            Frame::Enter(versions) => {
                let selection = select_one(&versions, loader, game)?;
                if !seen.insert(selection.project_id) {
                    continue;
                }
                stack.push(Frame::Finish(selection.planned));
                for (dep_project, version_id) in selection.deps.into_iter().rev() {
                    if seen.contains(&dep_project) {
                        continue;
                    }
                    if !valid_project_id(&dep_project) {
                        return Err(bad_request("Dependency id is not valid"));
                    }
                    let next = if let Some(version_id) = version_id.as_deref() {
                        if !valid_project_id(version_id) {
                            return Err(bad_request("Dependency version is not valid"));
                        }
                        let pinned: ProjectVersion = modrinth_get(
                            &format!("{MODRINTH_API}/version/{version_id}"),
                            &[],
                        )
                        .await?;
                        if version_fits(&pinned, loader, game) {
                            vec![pinned]
                        } else {
                            project_versions(&dep_project, loader, game).await?
                        }
                    } else {
                        project_versions(&dep_project, loader, game).await?
                    };
                    stack.push(Frame::Enter(next));
                }
            }
        }
    }
    unique_names(&planned)
}

async fn project_versions(
    project_id: &str,
    loader: &str,
    game: &str,
) -> Result<Vec<ProjectVersion>, Error> {
    let loaders = serde_json::json!([loader]).to_string();
    let games = serde_json::json!([game]).to_string();
    modrinth_get(
        &format!("{MODRINTH_API}/project/{project_id}/version"),
        &[
            ("loaders", loaders),
            ("game_versions", games),
            ("include_changelog", "false".to_string()),
        ],
    )
    .await
}

pub async fn download_jar(url: &str) -> Result<Vec<u8>, Error> {
    if !allowed_download_url(url) {
        return Err(bad_request("Mod download host is not allowed"));
    }
    let response = client()
        .get(url)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .context("Failed to download the mod")?
        .error_for_status()
        .context("Failed to download the mod")?;
    if let Some(length) = response.content_length() {
        if length == 0 || length as usize > MAX_JAR_BYTES {
            return Err(bad_request("Mod jar is too large"));
        }
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to download the mod")?;
        if body.len() + chunk.len() > MAX_JAR_BYTES {
            return Err(bad_request("Mod jar is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn modrinth_get<T: for<'de> Deserialize<'de>>(
    url: &str,
    query: &[(&str, String)],
) -> Result<T, Error> {
    let response = client()
        .get(url)
        .query(query)
        .send()
        .await
        .context("Modrinth request failed")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(not_found("Modrinth project was not found"));
    }
    let response = response
        .error_for_status()
        .context("Modrinth request failed")?;
    response
        .json()
        .await
        .context("Modrinth response was not valid")
        .map_err(Error::from)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!(
            "Lodestone/{} (https://github.com/bravo68web/lodestone)",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("modrinth http client")
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
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
        allowed_download_url, choose_version, list_mod_jars, plan_install, remove_mod_jar,
        safe_mod_filename, write_mod_jar, Dependency, ProjectVersion, VersionFile,
    };

    fn file(name: &str, primary: bool) -> VersionFile {
        VersionFile {
            url: format!("https://cdn.modrinth.com/data/{name}"),
            filename: name.to_string(),
            primary,
            file_type: None,
        }
    }

    fn version(
        project_id: &str,
        number: &str,
        channel: &str,
        published: &str,
        environment: &str,
        files: Vec<VersionFile>,
        dependencies: Vec<Dependency>,
    ) -> ProjectVersion {
        ProjectVersion {
            _id: format!("{project_id}-{number}"),
            project_id: project_id.to_string(),
            version_number: number.to_string(),
            date_published: published.to_string(),
            version_type: channel.to_string(),
            environment: environment.to_string(),
            loaders: vec!["fabric".to_string()],
            game_versions: vec!["1.20.1".to_string()],
            files,
            dependencies,
        }
    }

    #[test]
    fn a_release_is_chosen_over_a_newer_beta_and_client_only_builds() {
        let mut versions = vec![
            version(
                "sodium",
                "0.6.0-beta",
                "beta",
                "2026-02-01T00:00:00+00:00",
                "client_and_server",
                vec![file("sodium-beta.jar", true)],
                vec![],
            ),
            version(
                "sodium",
                "0.5.0",
                "release",
                "2026-01-01T00:00:00+00:00",
                "client_only",
                vec![file("sodium-client.jar", true)],
                vec![],
            ),
            version(
                "sodium",
                "0.5.1",
                "release",
                "2026-01-02T00:00:00+00:00",
                "client_and_server",
                vec![file("sodium-sources.jar", true), file("sodium.jar", false)],
                vec![],
            ),
        ];
        versions[2].files[0].file_type = Some("sources-jar".to_string());
        let chosen = choose_version(&versions, "fabric", "1.20.1").unwrap();
        assert_eq!(chosen.version_number, "0.5.1");
        assert_eq!(super::jar_file(chosen).unwrap().filename, "sodium.jar");
    }

    #[test]
    fn required_dependencies_are_installed_and_optional_ones_are_not() {
        let api = version(
            "fabric-api",
            "0.92.0",
            "release",
            "2026-01-01T00:00:00+00:00",
            "server_only",
            vec![file("fabric-api.jar", true)],
            vec![Dependency {
                version_id: None,
                project_id: Some("fabric-api".to_string()),
                dependency_type: "optional".to_string(),
            }],
        );
        let root = vec![version(
            "example",
            "1.0.0",
            "release",
            "2026-01-03T00:00:00+00:00",
            "client_and_server",
            vec![file("example.jar", true)],
            vec![
                Dependency {
                    version_id: None,
                    project_id: Some("fabric-api".to_string()),
                    dependency_type: "required".to_string(),
                },
                Dependency {
                    version_id: None,
                    project_id: Some("modmenu".to_string()),
                    dependency_type: "optional".to_string(),
                },
                Dependency {
                    version_id: None,
                    project_id: Some("embedded-lib".to_string()),
                    dependency_type: "embedded".to_string(),
                },
            ],
        )];
        let plan = plan_install(&root, "fabric", "1.20.1", &mut |project_id, _| {
            assert_eq!(project_id, "fabric-api");
            Ok(vec![api.clone()])
        })
        .unwrap();
        let names: Vec<&str> = plan.iter().map(|item| item.filename.as_str()).collect();
        assert_eq!(names, vec!["fabric-api.jar", "example.jar"]);
    }

    #[test]
    fn install_writes_the_jar_only_inside_mods_and_remove_deletes_that_jar() {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("instance");
        std::fs::create_dir_all(instance.join("world")).unwrap();
        std::fs::write(instance.join("world/level.dat"), b"world").unwrap();
        let jar = b"PK\x03\x04mod-bytes";
        write_mod_jar(&instance, "example.jar", jar).unwrap();
        write_mod_jar(&instance, "other.jar", jar).unwrap();
        assert_eq!(std::fs::read(instance.join("mods/example.jar")).unwrap(), jar);
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"world");
        assert!(safe_mod_filename("../evil.jar").is_err());
        assert!(safe_mod_filename("nested/evil.jar").is_err());
        assert!(!allowed_download_url("https://example.com/evil.jar"));
        assert!(allowed_download_url(
            "https://cdn.modrinth.com/data/AABBCCDD/versions/1.0.0/example.jar"
        ));
        let listed = list_mod_jars(&instance).unwrap();
        assert_eq!(listed.len(), 2);
        remove_mod_jar(&instance, "example.jar").unwrap();
        assert!(!instance.join("mods/example.jar").exists());
        assert!(instance.join("mods/other.jar").is_file());
        assert_eq!(std::fs::read(instance.join("world/level.dat")).unwrap(), b"world");
    }
}
