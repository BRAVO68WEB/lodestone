use color_eyre::eyre::eyre;

use crate::error::{Error, ErrorKind};
use crate::traits::t_server::PlayerLocation;

use super::MinecraftInstance;

const BIOMES: &[&str] = &[
    "plains",
    "sunflower_plains",
    "snowy_plains",
    "ice_spikes",
    "desert",
    "swamp",
    "mangrove_swamp",
    "forest",
    "flower_forest",
    "birch_forest",
    "dark_forest",
    "old_growth_birch_forest",
    "old_growth_pine_taiga",
    "old_growth_spruce_taiga",
    "taiga",
    "snowy_taiga",
    "savanna",
    "savanna_plateau",
    "windswept_hills",
    "windswept_gravelly_hills",
    "windswept_forest",
    "windswept_savanna",
    "jungle",
    "sparse_jungle",
    "bamboo_jungle",
    "badlands",
    "eroded_badlands",
    "wooded_badlands",
    "meadow",
    "cherry_grove",
    "grove",
    "snowy_slopes",
    "frozen_peaks",
    "jagged_peaks",
    "stony_peaks",
    "river",
    "frozen_river",
    "beach",
    "snowy_beach",
    "stony_shore",
    "warm_ocean",
    "lukewarm_ocean",
    "deep_lukewarm_ocean",
    "ocean",
    "deep_ocean",
    "cold_ocean",
    "deep_cold_ocean",
    "frozen_ocean",
    "deep_frozen_ocean",
    "mushroom_fields",
    "dripstone_caves",
    "lush_caves",
    "deep_dark",
    "pale_garden",
    "nether_wastes",
    "soul_sand_valley",
    "crimson_forest",
    "warped_forest",
    "basalt_deltas",
    "the_end",
    "small_end_islands",
    "end_midlands",
    "end_highlands",
    "end_barrens",
];

const PACK_MCMETA: &str = r#"{
  "pack": {
    "pack_format": 15,
    "supported_formats": { "min_inclusive": 10, "max_inclusive": 100 },
    "description": "Lodestone player biome lookup"
  }
}
"#;

pub fn valid_player_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=16).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

pub fn parse_entity_pos(reply: &str) -> Option<(f64, f64, f64)> {
    let start = reply.find('[')?;
    let end = reply[start + 1..].find(']')? + start + 1;
    let mut numbers = Vec::new();
    for part in reply[start + 1..end].split(',') {
        let trimmed = part.trim().trim_end_matches(['d', 'D', 'f', 'F']);
        numbers.push(trimmed.parse::<f64>().ok()?);
    }
    if numbers.len() != 3 || numbers.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some((round_1(numbers[0]), round_1(numbers[1]), round_1(numbers[2])))
}

pub fn parse_dimension(reply: &str) -> Option<String> {
    let id = last_quoted(reply).or_else(|| token_after_colon(reply))?;
    let short = id.strip_prefix("minecraft:").unwrap_or(id.as_str());
    if short.is_empty() || short == "unknown" {
        return None;
    }
    Some(match short {
        "overworld" => "overworld".to_string(),
        "the_nether" => "nether".to_string(),
        "the_end" => "end".to_string(),
        other => other.to_string(),
    })
}

pub fn parse_storage_string(reply: &str) -> Option<String> {
    let lower = reply.to_lowercase();
    if lower.contains("no data") || lower.contains("not found") {
        return None;
    }
    let value = last_quoted(reply)?;
    let short = value.strip_prefix("minecraft:").unwrap_or(value.as_str());
    if short.is_empty() || short == "unknown" {
        None
    } else {
        Some(short.to_string())
    }
}

fn round_1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn last_quoted(reply: &str) -> Option<String> {
    let mut last = None;
    let mut rest = reply;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let end = rest.find('"')?;
        last = Some(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    last
}

fn token_after_colon(reply: &str) -> Option<String> {
    let (_, after) = reply.rsplit_once(':')?;
    let token = after.split_whitespace().next()?.trim_matches(|c: char| {
        c == '"' || c == ',' || c == '[' || c == ']'
    });
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn biome_function() -> String {
    let mut body = String::from("data modify storage lodestone:where biome set value \"unknown\"\n");
    for biome in BIOMES {
        body.push_str("execute if biome ~ ~ ~ minecraft:");
        body.push_str(biome);
        body.push_str(" run data modify storage lodestone:where biome set value \"");
        body.push_str(biome);
        body.push_str("\"\n");
    }
    body
}

pub(crate) fn rcon_is_missing(error: &Error) -> bool {
    let text = error.to_string().to_lowercase();
    text.contains("not initialized") || text.contains("not connected")
}

impl MinecraftInstance {
    pub async fn where_player(&self, name: &str) -> Result<PlayerLocation, Error> {
        if !valid_player_name(name) {
            return Err(Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Invalid player name"),
            });
        }
        if self.rcon_conn.lock().await.is_none() {
            return Ok(PlayerLocation::unavailable(
                "Enable RCON in server properties to see dimension, coordinates, and biome.",
            ));
        }

        let pos_reply = self
            .send_rcon(&format!("data get entity {name} Pos"))
            .await;
        if let Err(error) = &pos_reply {
            if rcon_is_missing(error) {
                return Ok(PlayerLocation::unavailable(
                    "Enable RCON in server properties to see dimension, coordinates, and biome.",
                ));
            }
        }
        let dim_reply = self
            .send_rcon(&format!("data get entity {name} Dimension"))
            .await;

        let (x, y, z) = match pos_reply.as_ref().ok().and_then(|reply| parse_entity_pos(reply)) {
            Some((x, y, z)) => (Some(x), Some(y), Some(z)),
            None => (None, None, None),
        };
        let dimension = dim_reply
            .as_ref()
            .ok()
            .and_then(|reply| parse_dimension(reply))
            .unwrap_or_else(|| "unknown".to_string());
        let biome = self.lookup_biome(name).await;
        let message = if x.is_none() && dimension == "unknown" && biome == "unknown" {
            Some("Could not read that player. They may be offline.".to_string())
        } else {
            None
        };
        Ok(PlayerLocation {
            dimension,
            x,
            y,
            z,
            biome,
            rcon_connected: true,
            message,
        })
    }

    async fn lookup_biome(&self, name: &str) -> String {
        if self.ensure_where_pack().await.is_err() {
            return "unknown".to_string();
        }
        let ran = self
            .send_rcon(&format!(
                "execute as {name} at @s run function lodestone:biome"
            ))
            .await;
        let Ok(reply) = ran else {
            return "unknown".to_string();
        };
        let lower = reply.to_lowercase();
        if lower.contains("unknown function") || lower.contains("failed") || lower.contains("error")
        {
            return "unknown".to_string();
        }
        self.send_rcon("data get storage lodestone:where biome")
            .await
            .ok()
            .and_then(|reply| parse_storage_string(&reply))
            .unwrap_or_else(|| "unknown".to_string())
    }

    async fn ensure_where_pack(&self) -> Result<(), Error> {
        let mut ready = self.where_pack_ready.lock().await;
        if *ready {
            return Ok(());
        }
        let list = self.send_rcon("datapack list").await?;
        if datapack_enabled(&list) {
            *ready = true;
            return Ok(());
        }
        let world = world_directory(&self.path_to_instance, &self.path_to_properties).await;
        let Some(world) = world else {
            return Err(Error {
                kind: ErrorKind::Internal,
                source: eyre!("World directory is not created yet"),
            });
        };
        write_where_pack(&world).await?;
        self.send_rcon("reload").await?;
        let _ = self
            .send_rcon("datapack enable \"file/lodestone_where\"")
            .await;
        *ready = true;
        Ok(())
    }
}

fn datapack_enabled(list: &str) -> bool {
    let lower = list.to_lowercase();
    let enabled = lower.split("disabled").next().unwrap_or(&lower);
    enabled.contains("lodestone_where")
}

async fn world_directory(
    instance: &std::path::Path,
    properties: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut level = "world".to_string();
    if let Ok(text) = tokio::fs::read_to_string(properties).await {
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("level-name=") {
                let value = value.trim();
                if !value.is_empty()
                    && !value.contains("..")
                    && !value.contains('/')
                    && !value.contains('\\')
                {
                    level = value.to_string();
                }
            }
        }
    }
    let dir = instance.join(level);
    if tokio::fs::metadata(&dir).await.map(|meta| meta.is_dir()).unwrap_or(false) {
        Some(dir)
    } else {
        None
    }
}

async fn write_where_pack(world: &std::path::Path) -> Result<(), Error> {
    let root = world.join("datapacks").join("lodestone_where");
    let function = biome_function();
    let legacy = root.join("data").join("lodestone").join("functions");
    let modern = root.join("data").join("lodestone").join("function");
    tokio::fs::create_dir_all(&legacy)
        .await
        .map_err(|error| Error {
            kind: ErrorKind::Internal,
            source: eyre!(error),
        })?;
    tokio::fs::create_dir_all(&modern)
        .await
        .map_err(|error| Error {
            kind: ErrorKind::Internal,
            source: eyre!(error),
        })?;
    tokio::fs::write(root.join("pack.mcmeta"), PACK_MCMETA)
        .await
        .map_err(|error| Error {
            kind: ErrorKind::Internal,
            source: eyre!(error),
        })?;
    tokio::fs::write(legacy.join("biome.mcfunction"), &function)
        .await
        .map_err(|error| Error {
            kind: ErrorKind::Internal,
            source: eyre!(error),
        })?;
    tokio::fs::write(modern.join("biome.mcfunction"), function)
        .await
        .map_err(|error| Error {
            kind: ErrorKind::Internal,
            source: eyre!(error),
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_names_are_minecraft_safe() {
        assert!(valid_player_name("Steve"));
        assert!(valid_player_name("_x"));
        assert!(!valid_player_name(""));
        assert!(!valid_player_name("has space"));
        assert!(!valid_player_name("bad;name"));
        assert!(!valid_player_name(&"a".repeat(17)));
    }

    #[test]
    fn parses_position_and_rounds_to_one_decimal() {
        let reply = "Steve has the following entity data: [12.54d, 64.0d, -30.24d]";
        assert_eq!(parse_entity_pos(reply), Some((12.5, 64.0, -30.2)));
        assert_eq!(parse_entity_pos("No entity was found"), None);
    }

    #[test]
    fn parses_dimension_names() {
        assert_eq!(
            parse_dimension("Steve has the following entity data: \"minecraft:overworld\"").as_deref(),
            Some("overworld")
        );
        assert_eq!(
            parse_dimension("Steve has the following entity data: \"minecraft:the_nether\"").as_deref(),
            Some("nether")
        );
        assert_eq!(
            parse_dimension("Steve has the following entity data: \"minecraft:the_end\"").as_deref(),
            Some("end")
        );
        assert_eq!(parse_dimension("No entity was found"), None);
    }

    #[test]
    fn parses_biome_storage() {
        assert_eq!(
            parse_storage_string("Storage lodestone:where has the following contents: \"plains\"").as_deref(),
            Some("plains")
        );
        assert_eq!(
            parse_storage_string("Storage lodestone:where has the following contents: \"minecraft:dark_forest\"").as_deref(),
            Some("dark_forest")
        );
        assert_eq!(parse_storage_string("No data was found"), None);
        assert_eq!(
            parse_storage_string("Storage lodestone:where has the following contents: \"unknown\""),
            None
        );
    }
}
