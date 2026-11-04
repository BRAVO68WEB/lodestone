use color_eyre::eyre::eyre;

use crate::error::{Error, ErrorKind};
use crate::traits::t_server::{PlayerAction, PlayerInventory, PlayerItem};

use super::player_where::{rcon_is_missing, valid_player_name};
use super::MinecraftInstance;

pub fn parse_item_list(reply: &str) -> Vec<PlayerItem> {
    let lower = reply.to_lowercase();
    if lower.contains("no entity was found") || lower.contains("does not exist") {
        return Vec::new();
    }
    let start = reply.find('[').unwrap_or(0);
    let body = &reply[start..];
    let mut items = Vec::new();
    let mut depth = 0;
    let mut object_start = None;
    for (index, ch) in body.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    object_start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = object_start.take() {
                        if let Some(item) = parse_item(&body[start..=index]) {
                            items.push(item);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    items
}

fn parse_item(object: &str) -> Option<PlayerItem> {
    let id = quoted_after(object, "id:")?;
    let id = id.strip_prefix("minecraft:").unwrap_or(id.as_str());
    if id.is_empty() || id.len() > 64 || id.contains(' ') {
        return None;
    }
    let count = number_after(object, "count:").or_else(|| number_after(object, "Count:"))?;
    if count <= 0 {
        return None;
    }
    let slot = number_after(object, "Slot:").unwrap_or(0);
    Some(PlayerItem {
        slot: slot as i32,
        id: id.to_string(),
        count: count as u32,
    })
}

fn quoted_after(text: &str, key: &str) -> Option<String> {
    let index = text.find(key)?;
    let rest = text[index + key.len()..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = &rest[quote.len_utf8()..];
    let end = inner.find(quote)?;
    Some(inner[..end].to_string())
}

fn number_after(text: &str, key: &str) -> Option<i64> {
    let index = text.find(key)?;
    let rest = text[index + key.len()..].trim_start();
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch == '-' && digits.is_empty() {
            digits.push(ch);
            continue;
        }
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        break;
    }
    if digits.is_empty() || digits == "-" {
        return None;
    }
    digits.parse().ok()
}

pub fn parse_entity_number(reply: &str) -> Option<f64> {
    let marker = "entity data:";
    let index = reply.to_lowercase().find(marker)?;
    let rest = reply[index + marker.len()..].trim();
    let mut token = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && token.is_empty()) {
            token.push(ch);
        } else if token.is_empty() {
            continue;
        } else {
            break;
        }
    }
    let value = token.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

pub fn minecraft_item_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || !trimmed.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'.' | b'-')
        })
    {
        return None;
    }
    if trimmed.contains(':') {
        Some(trimmed.to_string())
    } else {
        Some(format!("minecraft:{trimmed}"))
    }
}

pub fn command_reason(raw: &str) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control() && *ch != '"' && *ch != '\\')
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn player_command(name: &str, action: &PlayerAction) -> Result<String, Error> {
    if !valid_player_name(name) {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Invalid player name"),
        });
    }
    Ok(match action {
        PlayerAction::Kick { reason } => {
            let reason = command_reason(reason);
            if reason.is_empty() {
                format!("kick {name}")
            } else {
                format!("kick {name} {reason}")
            }
        }
        PlayerAction::Clear { item: None } => format!("clear {name}"),
        PlayerAction::Clear { item: Some(item) } => {
            let id = minecraft_item_id(item).ok_or_else(|| Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Invalid item id"),
            })?;
            format!("clear {name} {id}")
        }
        PlayerAction::Ban { reason } => {
            let reason = command_reason(reason);
            if reason.is_empty() {
                format!("ban {name}")
            } else {
                format!("ban {name} {reason}")
            }
        }
        PlayerAction::WhitelistRemove => format!("whitelist remove {name}"),
    })
}

impl MinecraftInstance {
    pub async fn read_player_inventory(&self, name: &str) -> Result<PlayerInventory, Error> {
        if !valid_player_name(name) {
            return Err(Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Invalid player name"),
            });
        }
        if self.rcon_conn.lock().await.is_none() {
            return Ok(PlayerInventory::unavailable(
                "Enable RCON in server properties to see inventory and experience.",
            ));
        }
        let inventory_reply = self
            .send_rcon(&format!("data get entity {name} Inventory"))
            .await;
        if let Err(error) = &inventory_reply {
            if rcon_is_missing(error) {
                return Ok(PlayerInventory::unavailable(
                    "Enable RCON in server properties to see inventory and experience.",
                ));
            }
        }
        let ender_reply = self
            .send_rcon(&format!("data get entity {name} EnderItems"))
            .await
            .ok();
        let level_reply = self
            .send_rcon(&format!("data get entity {name} XpLevel"))
            .await
            .ok();
        let progress_reply = self
            .send_rcon(&format!("data get entity {name} XpP"))
            .await
            .ok();
        let inventory = inventory_reply
            .as_ref()
            .map(|reply| parse_item_list(reply))
            .unwrap_or_default();
        let ender_chest = ender_reply
            .as_ref()
            .map(|reply| parse_item_list(reply))
            .unwrap_or_default();
        let xp_level = level_reply
            .as_ref()
            .and_then(|reply| parse_entity_number(reply))
            .map(|value| value.max(0.0) as u32);
        let xp_progress = progress_reply
            .as_ref()
            .and_then(|reply| parse_entity_number(reply))
            .map(|value| value as f32);
        let message = if inventory_reply.is_err() && xp_level.is_none() {
            Some("Could not read that player. They may be offline.".to_string())
        } else {
            None
        };
        Ok(PlayerInventory {
            rcon_connected: true,
            message,
            xp_level,
            xp_progress,
            inventory,
            ender_chest,
        })
    }

    pub async fn run_player_action(
        &self,
        name: &str,
        action: PlayerAction,
    ) -> Result<String, Error> {
        let command = player_command(name, &action)?;
        if self.rcon_conn.lock().await.is_none() {
            return Err(Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Enable RCON in server properties to run player commands."),
            });
        }
        self.send_rcon(&command).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        minecraft_item_id, parse_entity_number, parse_item_list, player_command, PlayerAction,
    };

    #[test]
    fn inventory_keeps_old_and_new_count_fields() {
        let reply = r#"Alex has the following entity data: [{Slot: 0b, id: "minecraft:stone", Count: 64b}, {Slot: 9b, id: "minecraft:diamond_sword", count: 1}]"#;
        let items = parse_item_list(reply);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "stone");
        assert_eq!(items[0].count, 64);
        assert_eq!(items[0].slot, 0);
        assert_eq!(items[1].id, "diamond_sword");
        assert_eq!(items[1].count, 1);
    }

    #[test]
    fn an_empty_inventory_and_a_missing_player_have_no_items() {
        assert!(parse_item_list("Alex has the following entity data: []").is_empty());
        assert!(parse_item_list("No entity was found").is_empty());
    }

    #[test]
    fn experience_is_the_number_after_entity_data() {
        assert_eq!(
            parse_entity_number("Steve has the following entity data: 12"),
            Some(12.0)
        );
        assert_eq!(
            parse_entity_number("Steve has the following entity data: 0.5f"),
            Some(0.5)
        );
    }

    #[test]
    fn player_commands_stay_inside_the_name_and_item() {
        assert_eq!(
            player_command("Steve", &PlayerAction::Clear { item: None }).unwrap(),
            "clear Steve"
        );
        assert_eq!(
            player_command(
                "Steve",
                &PlayerAction::Clear {
                    item: Some("diamond".to_string())
                }
            )
            .unwrap(),
            "clear Steve minecraft:diamond"
        );
        assert!(player_command(
            "Steve",
            &PlayerAction::Clear {
                item: Some("dia mond".to_string())
            }
        )
        .is_err());
        assert_eq!(
            player_command(
                "Steve",
                &PlayerAction::Kick {
                    reason: "see you".to_string()
                }
            )
            .unwrap(),
            "kick Steve see you"
        );
        assert!(player_command("bad name!", &PlayerAction::WhitelistRemove).is_err());
        assert_eq!(minecraft_item_id("minecraft:oak_log").as_deref(), Some("minecraft:oak_log"));
    }
}
