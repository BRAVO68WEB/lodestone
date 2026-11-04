use color_eyre::eyre::{eyre, Context, ContextCompat};
use indexmap::IndexMap;
use serde_json::{self, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, str::FromStr};
use tokio::io::AsyncBufReadExt;

use super::{
    FabricInstallerVersion, FabricLoaderVersion, Flavour, ForgeBuildVersion, PaperBuildVersion,
    RestoreConfig,
};
use crate::error::{Error, ErrorKind};

pub async fn read_properties_from_path(
    path_to_properties: &Path,
) -> Result<IndexMap<String, String>, Error> {
    let properties_file = tokio::fs::File::open(path_to_properties)
        .await
        .context(format!(
            "Failed to open properties file at {}",
            path_to_properties.display()
        ))?;
    let buf_reader = tokio::io::BufReader::new(properties_file);
    let mut stream = buf_reader.lines();
    let mut ret = IndexMap::new();

    while let Some(line) = stream
        .next_line()
        .await
        .context("Failed to read line from properties file")?
    {
        if line.is_empty() {
            continue;
        }
        // if a line starts with '#', it is a comment, skip it
        if line.starts_with('#') {
            continue;
        }
        // split the line into key and value
        let mut split = line.split('=');
        let key = split
            .next()
            .ok_or_else(|| eyre!("Failed to read key from properties file"))?
            .trim();
        let value = split
            .next()
            .ok_or_else(|| eyre!("Failed to read value from properties file for key {}", key))?
            .trim();

        ret.insert(key.to_string(), value.to_string());
    }
    Ok(ret)
}

// Returns the jar url and the updated flavour with version information
pub async fn get_server_jar_url(version: &str, flavour: &Flavour) -> Option<(String, Flavour)> {
    match flavour {
        Flavour::Vanilla => get_vanilla_jar_url(version).await,
        Flavour::Fabric {
            loader_version,
            installer_version,
        } => get_fabric_jar_url(version, loader_version, installer_version).await,
        Flavour::Paper { build_version } => get_paper_jar_url(version, build_version).await,
        Flavour::Spigot => todo!(),
        Flavour::Forge { build_version } => get_forge_jar_url(version, build_version).await.ok(),
    }
}

pub async fn get_vanilla_jar_url(version: &str) -> Option<(String, Flavour)> {
    let client = reqwest::Client::new();
    let response_text = client
        .get("https://launchermeta.mojang.com/mc/game/version_manifest.json")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let response: serde_json::Value = serde_json::from_str(&response_text).ok()?;

    let url = response
        .get("versions")?
        .as_array()?
        .iter()
        .find(|version_json| {
            version_json
                .get("id")
                .unwrap()
                .as_str()
                .unwrap()
                .eq(version)
        })?
        .get("url")?
        .as_str()?;
    let response: serde_json::Value =
        serde_json::from_str(&client.get(url).send().await.ok()?.text().await.ok()?).ok()?;
    if response["downloads"]["server"]["url"] == serde_json::Value::Null {
        return None;
    }

    Some((
        response["downloads"]["server"]["url"]
            .to_string()
            .replace('\"', ""),
        Flavour::Vanilla,
    ))
}

pub async fn get_fabric_jar_url(
    version: &str,
    fabric_loader_version: &Option<FabricLoaderVersion>,
    fabric_installer_version: &Option<FabricInstallerVersion>,
) -> Option<(String, Flavour)> {
    let mut loader_version = String::new();
    let mut installer_version = String::new();
    let client = reqwest::Client::new();

    if let (Some(FabricLoaderVersion(l)), Some(FabricInstallerVersion(i))) =
        (fabric_loader_version, fabric_installer_version)
    {
        loader_version = l.to_string();
        installer_version = i.to_string();
        return Some((
            format!(
                "https://meta.fabricmc.net/v2/versions/loader/{}/{}/{}/server/jar",
                version, loader_version, installer_version
            ),
            Flavour::Fabric {
                loader_version: Some(FabricLoaderVersion(loader_version)),
                installer_version: Some(FabricInstallerVersion(installer_version)),
            },
        ));
    }

    if fabric_loader_version.is_none() {
        loader_version = serde_json::Value::from_str(
            client
                .get(format!(
                    "https://meta.fabricmc.net/v2/versions/loader/{}",
                    version
                ))
                .send()
                .await
                .ok()?
                .text()
                .await
                .ok()?
                .as_str(),
        )
        .ok()?
        .as_array()?
        .iter()
        .filter(|v| {
            v.get("loader")
                .unwrap()
                .get("stable")
                .unwrap()
                .as_bool()
                .unwrap()
                && v.get("intermediary")
                    .unwrap()
                    .get("stable")
                    .unwrap()
                    .as_bool()
                    .unwrap()
        })
        .max_by(|a, b| {
            let a_version = a
                .get("loader")
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap()
                .split('.')
                .collect::<Vec<&str>>();
            let b_version = b
                .get("loader")
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap()
                .split('.')
                .collect::<Vec<&str>>();
            for (a_part, b_part) in a_version.iter().zip(b_version.iter()) {
                if a_part.parse::<i32>().unwrap() > b_part.parse::<i32>().unwrap() {
                    return std::cmp::Ordering::Greater;
                } else if a_part.parse::<i32>().unwrap() < b_part.parse::<i32>().unwrap() {
                    return std::cmp::Ordering::Less;
                }
            }
            std::cmp::Ordering::Equal
        })?
        .get("loader")?
        .get("version")?
        .as_str()?
        .to_string();
    }

    if fabric_installer_version.is_none() {
        installer_version = serde_json::Value::from_str(
            client
                .get("https://meta.fabricmc.net/v2/versions/installer")
                .send()
                .await
                .ok()?
                .text()
                .await
                .ok()?
                .as_str(),
        )
        .ok()?
        .as_array()?
        .iter()
        .filter(|v| v.get("stable").unwrap().as_bool().unwrap())
        .max_by(|a, b| {
            // sort the version string in the form of "1.2.3"
            let a_version = a
                .get("loader")
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap()
                .split('.')
                .collect::<Vec<&str>>();
            let b_version = b
                .get("loader")
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap()
                .split('.')
                .collect::<Vec<&str>>();
            for (a_part, b_part) in a_version.iter().zip(b_version.iter()) {
                if a_part.parse::<i32>().unwrap() > b_part.parse::<i32>().unwrap() {
                    return std::cmp::Ordering::Greater;
                } else if a_part.parse::<i32>().unwrap() < b_part.parse::<i32>().unwrap() {
                    return std::cmp::Ordering::Less;
                }
            }
            std::cmp::Ordering::Equal
        })?
        .get("version")?
        .as_str()?
        .to_string();
    }
    Some((
        format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{}/{}/server/jar",
            version, loader_version, installer_version
        ),
        Flavour::Fabric {
            loader_version: Some(FabricLoaderVersion(loader_version)),
            installer_version: Some(FabricInstallerVersion(installer_version)),
        },
    ))
}

pub(super) fn paper_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!(
            "Lodestone/{} (https://github.com/bravo68web/lodestone)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .expect("paper http client")
}

/// Pick a Fill v3 build. `builds` is newest-first. A missing stable build returns None.
pub fn select_paper_download(
    builds: &[Value],
    paper_build_version: &Option<PaperBuildVersion>,
) -> Option<(String, Flavour)> {
    let build = if let Some(PaperBuildVersion(b)) = paper_build_version {
        builds
            .iter()
            .find(|build| build.get("id").and_then(|id| id.as_i64()) == Some(*b))?
    } else {
        builds
            .iter()
            .find(|build| build.get("channel").and_then(|channel| channel.as_str()) == Some("STABLE"))?
    };
    let build_version = build.get("id")?.as_i64()?;
    let url = build
        .get("downloads")?
        .get("server:default")?
        .get("url")?
        .as_str()?
        .to_string();
    Some((
        url,
        Flavour::Paper {
            build_version: Some(PaperBuildVersion(build_version)),
        },
    ))
}

pub async fn get_paper_jar_url(
    version: &str,
    paper_build_version: &Option<PaperBuildVersion>,
) -> Option<(String, Flavour)> {
    let client = paper_http_client();
    let builds: Vec<Value> = client
        .get(format!(
            "https://fill.papermc.io/v3/projects/paper/versions/{}/builds",
            version
        ))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    select_paper_download(&builds, paper_build_version)
}

pub async fn get_forge_jar_url(
    version: &str,
    forge_build_version: &Option<ForgeBuildVersion>,
) -> Result<(String, Flavour), Error> {
    let client = reqwest::Client::new();

    let response: BTreeMap<String, Vec<String>> = serde_json::from_str(
        client
            .get("https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json")
            .send()
            .await
            .context("Failed to get forge versions, http request failed")?
            .text()
            .await
            .context("Failed to get forge versions, text conversion failed")?
            .as_str(),
    )
    .context("Failed to get forge versions, json is not a map")?;

    let build = if let Some(ForgeBuildVersion(b)) = forge_build_version {
        b
    } else {
        response
            .get(version)
            .context("Failed to get forge versions, version not found")?
            .last()
            .context("Failed to get forge versions, no builds found")?
    };

    Ok((
        format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{}/forge-{}-installer.jar",
            build, build
        ),
        Flavour::Forge {
            build_version: Some(ForgeBuildVersion(build.to_string())),
        },
    ))
}

pub async fn get_jre_url(version: &str) -> Option<(String, u64)> {
    let client = reqwest::Client::new();
    let os = if std::env::consts::OS == "macos" {
        "mac"
    } else {
        std::env::consts::OS
    };
    let arch = if std::env::consts::ARCH == "x86_64" {
        "x64"
    } else {
        std::env::consts::ARCH
    };

    let major_java_version = {
        let val = match serde_json::Value::from_str(
            client
                .get(
                    serde_json::Value::from_str(
                        client
                            .get("https://launchermeta.mojang.com/mc/game/version_manifest.json")
                            .send()
                            .await
                            .ok()?
                            .text()
                            .await
                            .ok()?
                            .as_str(),
                    )
                    .ok()?
                    .get("versions")?
                    .as_array()?
                    .iter()
                    .find(|v| v.get("id").unwrap().as_str().unwrap().eq(version))?
                    .get("url")?
                    .as_str()?,
                )
                .send()
                .await
                .ok()?
                .text()
                .await
                .ok()?
                .as_str(),
        )
        .ok()?
        .get("javaVersion")
        {
            Some(java_version) => java_version.get("majorVersion")?.as_u64()?,
            None => 8,
        };
        // Ddoptium won't provide java 16 for some reason
        // updateing to 17 should be safe, and 17 is preferred since its LTS
        if val == 16 {
            17
        } else {
            val
        }
    };

    Some((
        format!(
            "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jre/hotspot/normal/eclipse",
            major_java_version, os, arch
        ),
        major_java_version,
    ))
}

pub async fn name_to_uuid(name: impl AsRef<str>) -> Option<String> {
    // GET https://api.mojang.com/users/profiles/minecraft/<username>
    let client = reqwest::Client::new();
    let res: Value = client
        .get(format!(
            "https://api.mojang.com/users/profiles/minecraft/{}",
            name.as_ref()
        ))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    Some(res["id"].as_str()?.to_owned())
}

const MAX_VERSION_JSON_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftJarInfo {
    pub version: String,
    pub java_major: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenJava {
    pub binary: PathBuf,
    pub major: u64,
    pub game_version: Option<String>,
    pub from_jar: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaResolution {
    pub binary: PathBuf,
    pub changed: bool,
}

pub fn java_runtime_binary(runtimes: &Path, major: u64) -> PathBuf {
    runtimes
        .join("java")
        .join(format!("jre{major}"))
        .join(if std::env::consts::OS == "macos" {
            "Contents/Home/bin"
        } else {
            "bin"
        })
        .join("java")
}

fn is_safe_game_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && !version.contains("..")
        && version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_')
}

pub fn parse_minecraft_version_json(text: &str) -> Option<MinecraftJarInfo> {
    let value: Value = serde_json::from_str(text).ok()?;
    let version = value.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if !is_safe_game_version(version) {
        return None;
    }
    let java_major = value
        .get("java_version")
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("javaVersion")
                .and_then(|java| java.get("majorVersion"))
                .and_then(Value::as_u64)
        })
        .filter(|major| (8..=64).contains(major))?;
    Some(MinecraftJarInfo {
        version: version.to_string(),
        java_major,
    })
}

/// Read `version.json` from the root of a server jar. A paperclip jar, including Purpur, keeps it there.
pub fn read_minecraft_jar_info(jar: &Path) -> Option<MinecraftJarInfo> {
    let file = std::fs::File::open(jar).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("version.json").ok()?;
    if entry.size() > MAX_VERSION_JSON_BYTES {
        return None;
    }
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    parse_minecraft_version_json(&text)
}

fn runtime_if_present(runtimes: &Path, major: u64) -> Option<PathBuf> {
    let path = java_runtime_binary(runtimes, major);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn missing_java(major: u64) -> Error {
    Error {
        kind: ErrorKind::BadRequest,
        source: eyre!("This server needs Java {major}, and that runtime is not installed"),
    }
}

pub fn choose_java_runtime(
    runtimes: &Path,
    jar: Option<&MinecraftJarInfo>,
    configured_major: u64,
    configured_version: &str,
) -> Result<ChosenJava, Error> {
    let imported = configured_version.is_empty() || configured_version == "imported";
    let configured = runtime_if_present(runtimes, configured_major);

    if let Some(info) = jar {
        if let Some(binary) = runtime_if_present(runtimes, info.java_major) {
            let use_jar = imported || configured.is_none() || info.java_major == configured_major;
            if use_jar {
                return Ok(ChosenJava {
                    binary,
                    major: info.java_major,
                    game_version: if imported {
                        Some(info.version.clone())
                    } else {
                        None
                    },
                    from_jar: true,
                });
            }
        } else if imported || configured.is_none() {
            return Err(missing_java(info.java_major));
        }
    }

    if let Some(binary) = configured {
        return Ok(ChosenJava {
            binary,
            major: configured_major,
            game_version: None,
            from_jar: false,
        });
    }

    let major = jar
        .map(|info| info.java_major)
        .unwrap_or(configured_major);
    Err(missing_java(major))
}

fn existing_java_cmd(java_cmd: Option<&str>) -> Option<PathBuf> {
    let command = java_cmd.map(str::trim).filter(|command| !command.is_empty())?;
    let path = PathBuf::from(command);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Pick a Java binary that exists. An imported server follows `version.json` instead of the default Java 17.
pub fn resolve_instance_java(
    config: &mut RestoreConfig,
    runtimes: &Path,
    instance: &Path,
) -> Result<JavaResolution, Error> {
    if let Some(binary) = existing_java_cmd(config.java_cmd.as_deref()) {
        return Ok(JavaResolution {
            binary,
            changed: false,
        });
    }
    let jar = read_minecraft_jar_info(&instance.join("server.jar"));
    let chosen = choose_java_runtime(
        runtimes,
        jar.as_ref(),
        config.jre_major_version,
        &config.version,
    )?;
    let mut changed = false;
    if chosen.from_jar {
        let binary = chosen.binary.to_string_lossy().into_owned();
        if config.java_cmd.as_deref() != Some(binary.as_str()) {
            config.java_cmd = Some(binary);
            changed = true;
        }
        if config.jre_major_version != chosen.major {
            config.jre_major_version = chosen.major;
            changed = true;
        }
        if let Some(version) = chosen.game_version {
            if config.version != version {
                config.version = version;
                changed = true;
            }
        }
    }
    Ok(JavaResolution {
        binary: chosen.binary,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use crate::minecraft::{
        util::{get_forge_jar_url, get_server_jar_url},
        FabricInstallerVersion, FabricLoaderVersion, Flavour, ForgeBuildVersion, PaperBuildVersion,
    };
    use tokio;

    #[tokio::test]
    async fn test_get_vanilla_jar_url() {
        assert_eq!(super::get_vanilla_jar_url("1.18.2").await, Some(("https://piston-data.mojang.com/v1/objects/c8f83c5655308435b3dcf03c06d9fe8740a77469/server.jar".to_string(), Flavour::Vanilla)));
        assert_eq!(super::get_vanilla_jar_url("21w44a").await, Some(("https://piston-data.mojang.com/v1/objects/ae583fd57a8c07f2d6fbadce1ce1e1379bf4b32d/server.jar".to_string(), Flavour::Vanilla)));
        assert_eq!(super::get_vanilla_jar_url("1.8.4").await, Some(("https://launcher.mojang.com/v1/objects/dd4b5eba1c79500390e0b0f45162fa70d38f8a3d/server.jar".to_string(), Flavour::Vanilla)));

        assert_eq!(super::get_vanilla_jar_url("1.8.4asdasd").await, None);
    }
    #[tokio::test]
    async fn test_get_jre_url() {
        // let os_str = if std::env::consts::OS == "macos" {
        //     "mac"
        // } else {
        //     std::env::consts::OS
        // };
        // TODO: Make this test more robust
        // assert_eq!(super::get_jre_url("1.18.2").await, Some((format!("https://api.adoptium.net/v3/binary/latest/17/ga/{os_str}/x64/jre/hotspot/normal/eclipse"), 17)));
        // assert_eq!(super::get_jre_url("21w44a").await, Some((format!("https://api.adoptium.net/v3/binary/latest/17/ga/{os_str}/x64/jre/hotspot/normal/eclipse"), 17)));
        // assert_eq!(super::get_jre_url("1.8.4").await, Some((format!("https://api.adoptium.net/v3/binary/latest/8/ga/{os_str}/x64/jre/hotspot/normal/eclipse"), 8)));

        assert_eq!(super::get_jre_url("1.8.4asdasd").await, None);
    }

    /// Test subject to fail if fabric updates their installer or loader
    #[tokio::test]
    async fn test_get_fabric_jar_url() {
        assert_eq!(
            super::get_fabric_jar_url(
                "1.19",
                &Some(FabricLoaderVersion("0.14.8".to_string())),
                &Some(FabricInstallerVersion("0.11.0".to_string()))
            )
            .await,
            Some((
                "https://meta.fabricmc.net/v2/versions/loader/1.19/0.14.8/0.11.0/server/jar"
                    .to_string(),
                Flavour::Fabric {
                    loader_version: Some(FabricLoaderVersion("0.14.8".to_string())),
                    installer_version: Some(FabricInstallerVersion("0.11.0".to_string()))
                }
            ))
        );
        assert!(super::get_fabric_jar_url("21w44a", &None, &None)
            .await
            .is_some());
    }

    #[test]
    fn test_select_paper_download() {
        let builds = serde_json::json!([
            {
                "id": 48,
                "channel": "STABLE",
                "downloads": {
                    "server:default": {
                        "name": "paper-1.21.1-48.jar",
                        "url": "https://fill-data.papermc.io/v1/objects/abc/paper-1.21.1-48.jar"
                    }
                }
            },
            {
                "id": 47,
                "channel": "ALPHA",
                "downloads": {
                    "server:default": {
                        "url": "https://fill-data.papermc.io/v1/objects/def/paper-1.21.1-47.jar"
                    }
                }
            }
        ]);
        let builds = builds.as_array().unwrap();
        assert_eq!(
            super::select_paper_download(builds, &None),
            Some((
                "https://fill-data.papermc.io/v1/objects/abc/paper-1.21.1-48.jar".to_string(),
                Flavour::Paper {
                    build_version: Some(PaperBuildVersion(48))
                }
            ))
        );
        assert_eq!(
            super::select_paper_download(builds, &Some(PaperBuildVersion(47))),
            Some((
                "https://fill-data.papermc.io/v1/objects/def/paper-1.21.1-47.jar".to_string(),
                Flavour::Paper {
                    build_version: Some(PaperBuildVersion(47))
                }
            ))
        );
        let alpha_only = serde_json::json!([
            {"id": 1, "channel": "ALPHA", "downloads": {"server:default": {"url": "https://example.invalid/a.jar"}}}
        ]);
        assert_eq!(
            super::select_paper_download(alpha_only.as_array().unwrap(), &None),
            None
        );
    }

    #[tokio::test]
    async fn test_get_forge_jar_url() {
        get_forge_jar_url("1.18.2", &None).await.unwrap();
    }

    #[tokio::test]
    async fn test_get_server_jar_url() {
        assert_eq!(
            get_server_jar_url("1.7.10", &Flavour::Forge { build_version: None }).await,
            Some((
                "https://maven.minecraftforge.net/net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10-installer.jar".to_string(),
                Flavour::Forge { build_version: Some(ForgeBuildVersion("1.7.10-10.13.4.1614-1.7.10".to_string())) }
            ))
        );
        assert_eq!(
            get_server_jar_url("1.7.10_pre4", &Flavour::Forge { build_version: None }).await,
            Some((
                "https://maven.minecraftforge.net/net/minecraftforge/forge/1.7.10_pre4-10.12.2.1149-prerelease/forge-1.7.10_pre4-10.12.2.1149-prerelease-installer.jar".to_string(),
                Flavour::Forge { build_version: Some(ForgeBuildVersion("1.7.10_pre4-10.12.2.1149-prerelease".to_string())) }
            ))
        );
        assert_eq!(
            get_server_jar_url(
                "1.19.3bruh",
                &Flavour::Forge {
                    build_version: None
                }
            )
            .await,
            None
        );
    }
}

#[cfg(test)]
mod java_for_server_tests {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use super::{
        choose_java_runtime, parse_minecraft_version_json, read_minecraft_jar_info,
        resolve_instance_java, MinecraftJarInfo,
    };
    use crate::implementations::minecraft::{Flavour, RestoreConfig};

    fn touch_runtime(root: &Path, major: u64) -> PathBuf {
        let path = super::java_runtime_binary(root, major);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"java").unwrap();
        path
    }

    fn write_server_jar(instance: &Path, body: &str) {
        std::fs::create_dir_all(instance).unwrap();
        let jar = instance.join("server.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
        zip.start_file("version.json", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn imported_config() -> RestoreConfig {
        RestoreConfig {
            name: "mc-26.3".to_string(),
            version: "imported".to_string(),
            flavour: Flavour::Vanilla,
            description: String::new(),
            cmd_args: Vec::new(),
            java_cmd: None,
            port: 25565,
            min_ram: 1024,
            max_ram: 2048,
            auto_start: false,
            restart_on_crash: false,
            backup_period: None,
            jre_major_version: 17,
            has_started: true,
        }
    }

    #[test]
    fn java_for_server_reads_version_json() {
        let info = parse_minecraft_version_json(
            r#"{"id":"26.3","java_version":25,"java_component":"java-runtime-epsilon"}"#,
        )
        .unwrap();
        assert_eq!(
            info,
            MinecraftJarInfo {
                version: "26.3".to_string(),
                java_major: 25,
            }
        );
        assert!(parse_minecraft_version_json(r#"{"id":"../evil","java_version":25}"#).is_none());
        let nested = parse_minecraft_version_json(
            r#"{"id":"1.20.4","javaVersion":{"majorVersion":21}}"#,
        )
        .unwrap();
        assert_eq!(nested.java_major, 21);

        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("instance");
        write_server_jar(&instance, r#"{"id":"26.3","java_version":25}"#);
        let from_jar = read_minecraft_jar_info(&instance.join("server.jar")).unwrap();
        assert_eq!(from_jar.version, "26.3");
        assert_eq!(from_jar.java_major, 25);
    }

    #[test]
    fn java_for_server_imported_picks_installed_major() {
        let root = tempfile::tempdir().unwrap();
        let jre25 = touch_runtime(root.path(), 25);
        let _jre17 = touch_runtime(root.path(), 17);
        let instance = root.path().join("instance");
        write_server_jar(&instance, r#"{"id":"26.3","java_version":25}"#);
        let mut config = imported_config();
        let resolved = resolve_instance_java(&mut config, root.path(), &instance).unwrap();
        assert!(resolved.changed);
        assert_eq!(resolved.binary, jre25);
        assert_eq!(config.jre_major_version, 25);
        assert_eq!(config.version, "26.3");
        assert_eq!(config.java_cmd.as_deref(), Some(jre25.to_str().unwrap()));
    }

    #[test]
    fn java_for_server_explicit_command_wins() {
        let root = tempfile::tempdir().unwrap();
        let _jre25 = touch_runtime(root.path(), 25);
        let instance = root.path().join("instance");
        write_server_jar(&instance, r#"{"id":"26.3","java_version":25}"#);
        let custom = root.path().join("custom-java");
        std::fs::write(&custom, b"java").unwrap();
        let mut config = imported_config();
        config.java_cmd = Some(custom.to_string_lossy().into_owned());
        let resolved = resolve_instance_java(&mut config, root.path(), &instance).unwrap();
        assert!(!resolved.changed);
        assert_eq!(resolved.binary, custom);
        assert_eq!(config.version, "imported");
        assert_eq!(config.jre_major_version, 17);
    }

    #[test]
    fn java_for_server_missing_runtime_is_clear() {
        let root = tempfile::tempdir().unwrap();
        let _jre17 = touch_runtime(root.path(), 17);
        let instance = root.path().join("instance");
        write_server_jar(&instance, r#"{"id":"26.3","java_version":25}"#);
        let mut config = imported_config();
        let err = resolve_instance_java(&mut config, root.path(), &instance).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("Java 25"), "{text}");
        assert!(text.contains("not installed"), "{text}");
        assert_eq!(config.jre_major_version, 17);
    }

    #[test]
    fn java_for_server_configured_runtime_wins_when_version_is_known() {
        let root = tempfile::tempdir().unwrap();
        let jre17 = touch_runtime(root.path(), 17);
        let _jre25 = touch_runtime(root.path(), 25);
        let jar = MinecraftJarInfo {
            version: "26.3".to_string(),
            java_major: 25,
        };
        let chosen = choose_java_runtime(root.path(), Some(&jar), 17, "1.21.4").unwrap();
        assert_eq!(chosen.binary, jre17);
        assert!(!chosen.from_jar);
        assert_eq!(chosen.major, 17);
    }
}
