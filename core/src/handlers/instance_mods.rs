use axum::extract::{Path, Query};
use axum::routing::{delete, get};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};

use crate::auth::user::UserAction;
use crate::error::{Error, ErrorKind};
use crate::minecraft::modrinth::{self, ModJar, ModProject, PlannedMod};
use crate::prelude::GameInstance;
use crate::traits::t_configurable::{Game, MinecraftVariant, TConfigurable};
use crate::traits::t_server::{State, TServer};
use crate::types::InstanceUuid;
use crate::AppState;

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Deserialize)]
struct InstallRequest {
    project_id: String,
}

#[derive(Serialize)]
struct InstalledMod {
    filename: String,
    project_id: String,
    version: String,
}

fn unsupported(message: &str) -> Error {
    Error {
        kind: ErrorKind::UnsupportedOperation,
        source: eyre!("{message}"),
    }
}

fn not_found() -> Error {
    Error {
        kind: ErrorKind::NotFound,
        source: eyre!("Instance not found"),
    }
}

fn stopped_error() -> Error {
    Error {
        kind: ErrorKind::BadRequest,
        source: eyre!("Stop the server before changing mods"),
    }
}

async fn authorize(state: &AppState, token: &str, action: &UserAction) -> Result<(), Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(token)?;
    requester.try_action(action, state.global_settings.lock().await.safe_mode())?;
    Ok(())
}

fn open_instance(state: &AppState, uuid: &InstanceUuid) -> Result<GameInstance, Error> {
    if uuid.to_string().starts_with("DOCKER-") {
        return Err(unsupported("Docker instances have no mods folder"));
    }
    state
        .instances
        .get(uuid)
        .map(|instance| instance.clone())
        .ok_or_else(not_found)
}

async fn loader_and_game(instance: &GameInstance) -> Result<(String, String), Error> {
    let loader = match instance.game_type().await {
        Game::MinecraftJava {
            variant: MinecraftVariant::Fabric,
        } => "fabric",
        Game::MinecraftJava {
            variant: MinecraftVariant::Forge,
        } => "forge",
        Game::MinecraftJava { .. } => {
            return Err(unsupported("This loader does not use a mods folder"))
        }
        _ => return Err(unsupported("This instance has no mods folder")),
    };
    let game = instance.version().await;
    if !modrinth::valid_game_version(&game) {
        return Err(Error {
            kind: ErrorKind::BadRequest,
            source: eyre!("Set a Minecraft version before installing mods"),
        });
    }
    Ok((loader.to_string(), game))
}

async fn list_mods(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<Vec<ModJar>>, Error> {
    authorize(&state, &token, &UserAction::ReadInstanceFile(uuid.clone())).await?;
    let instance = open_instance(&state, &uuid)?;
    let path = instance.path().await;
    Ok(Json(modrinth::list_mod_jars(&path)?))
}

async fn search_mods(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    Query(query): Query<SearchQuery>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<Vec<ModProject>>, Error> {
    authorize(&state, &token, &UserAction::ReadInstanceFile(uuid.clone())).await?;
    let instance = open_instance(&state, &uuid)?;
    let (loader, game) = loader_and_game(&instance).await?;
    Ok(Json(modrinth::search_projects(&query.q, &loader, &game).await?))
}

async fn install_mod(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(body): Json<InstallRequest>,
) -> Result<Json<Vec<InstalledMod>>, Error> {
    authorize(
        &state,
        &token,
        &UserAction::WriteInstanceFile(uuid.clone()),
    )
    .await?;
    let instance = open_instance(&state, &uuid)?;
    if instance.state().await != State::Stopped {
        return Err(stopped_error());
    }
    let (loader, game) = loader_and_game(&instance).await?;
    let plan = modrinth::plan_project(&body.project_id, &loader, &game).await?;
    if instance.state().await != State::Stopped {
        return Err(stopped_error());
    }
    let mut downloaded = Vec::new();
    for item in &plan {
        downloaded.push((item, modrinth::download_jar(&item.url).await?));
    }
    if instance.state().await != State::Stopped {
        return Err(stopped_error());
    }
    let path = instance.path().await;
    let mut created: Vec<String> = Vec::new();
    let mut installed = Vec::new();
    for (item, bytes) in downloaded {
        let jar_path = path.join("mods").join(&item.filename);
        let existed = jar_path.is_file();
        if let Err(err) = modrinth::write_mod_jar(&path, &item.filename, &bytes) {
            for name in &created {
                let _ = modrinth::remove_mod_jar(&path, name);
            }
            return Err(err);
        }
        if !existed {
            created.push(item.filename.clone());
        }
        installed.push(installed_from(item));
    }
    Ok(Json(installed))
}

fn installed_from(item: &PlannedMod) -> InstalledMod {
    InstalledMod {
        filename: item.filename.clone(),
        project_id: item.project_id.clone(),
        version: item.version.clone(),
    }
}

async fn remove_mod(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, name)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<()>, Error> {
    authorize(
        &state,
        &token,
        &UserAction::WriteInstanceFile(uuid.clone()),
    )
    .await?;
    let instance = open_instance(&state, &uuid)?;
    if instance.state().await != State::Stopped {
        return Err(stopped_error());
    }
    let path = instance.path().await;
    modrinth::remove_mod_jar(&path, &name)?;
    Ok(Json(()))
}

pub fn get_instance_mod_routes(state: AppState) -> Router {
    Router::new()
        .route("/instance/:uuid/mods", get(list_mods).post(install_mod))
        .route("/instance/:uuid/mods/search", get(search_mods))
        .route("/instance/:uuid/mods/:name", delete(remove_mod))
        .with_state(state)
}
