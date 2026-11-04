use std::collections::HashSet;

use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;

use serde::Deserialize;

use crate::{
    auth::user::UserAction,
    error::{Error, ErrorKind},
    implementations::minecraft::player_where::valid_player_name,
    traits::t_player::{Player, TPlayerManagement},
    traits::t_server::{PlayerAction, PlayerInventory, PlayerLocation, TServer},
    types::InstanceUuid,
    AppState,
};

pub async fn get_player_count(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
) -> Result<Json<u32>, Error> {
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .get_player_count()
        .await
        .map(Json)
}

pub async fn get_max_player_count(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
) -> Result<Json<u32>, Error> {
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .get_max_player_count()
        .await
        .map(Json)
}

pub async fn set_max_player_count(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    Json(count): Json<u32>,
) -> Result<Json<()>, Error> {
    state
        .instances
        .get_mut(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .set_max_player_count(count)
        .await
        .map(Json)
}

pub async fn get_player_list(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
) -> Result<Json<HashSet<Player>>, Error> {
    state
        .instances
        .get_mut(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .get_player_list()
        .await
        .map(Json)
}

pub async fn get_player_where(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, name)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<PlayerLocation>, Error> {
    if !valid_player_name(&name) {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Invalid player name"),
        });
    }
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::ViewInstance(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    instance.player_where(&name).await.map(Json)
}

pub async fn get_player_inventory(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, name)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<PlayerInventory>, Error> {
    if !valid_player_name(&name) {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Invalid player name"),
        });
    }
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::ViewInstance(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    instance.player_inventory(&name).await.map(Json)
}

#[derive(Deserialize)]
pub struct PlayerActionRequest {
    pub action: String,
    pub reason: Option<String>,
    pub item: Option<String>,
}

pub async fn post_player_action(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, name)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
    Json(body): Json<PlayerActionRequest>,
) -> Result<Json<String>, Error> {
    if !valid_player_name(&name) {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Invalid player name"),
        });
    }
    let action = match body.action.as_str() {
        "kick" => PlayerAction::Kick {
            reason: body.reason.unwrap_or_default(),
        },
        "clear" => PlayerAction::Clear { item: body.item },
        "ban" => PlayerAction::Ban {
            reason: body.reason.unwrap_or_default(),
        },
        "whitelist_remove" => PlayerAction::WhitelistRemove,
        _ => {
            return Err(Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Unknown player action"),
            })
        }
    };
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessConsole(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    instance.player_action(&name, action).await.map(Json)
}

pub fn get_instance_players_routes(state: AppState) -> Router {
    Router::new()
        .route("/instance/:uuid/players/count", get(get_player_count))
        .route(
            "/instance/:uuid/players/max",
            get(get_max_player_count).put(set_max_player_count),
        )
        .route("/instance/:uuid/players", get(get_player_list))
        .route("/instance/:uuid/player/:name/where", get(get_player_where))
        .route(
            "/instance/:uuid/player/:name/inventory",
            get(get_player_inventory),
        )
        .route(
            "/instance/:uuid/player/:name/action",
            post(post_player_action),
        )
        .with_state(state)
}
