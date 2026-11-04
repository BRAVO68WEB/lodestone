use axum::{
    extract::Path,
    routing::{get, put},
    Json, Router,
};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;

use serde::Deserialize;

use crate::{
    auth::user::UserAction,
    error::{Error, ErrorKind},
    implementations::minecraft::loader_update::LoaderUpdate,
    prelude::GameInstance,
    traits::t_configurable::{
        manifest::{ConfigurableManifest, ConfigurableValue},
        TConfigurable,
    },
    types::InstanceUuid,
    AppState,
};

pub async fn get_instance_configurable_manifest(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<ConfigurableManifest>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    let instance = state.instances.get(&uuid).ok_or_else(|| Error {
        kind: ErrorKind::NotFound,
        source: eyre!("Instance not found"),
    })?;
    Ok(Json(instance.configurable_manifest().await))
}

pub async fn get_instance_settings(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<ConfigurableManifest>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    if uuid.to_string().starts_with("DOCKER-") {
        return Ok(Json(ConfigurableManifest::default()));
    }
    let instance = state.instances.get(&uuid).ok_or_else(|| Error {
        kind: ErrorKind::NotFound,
        source: eyre!("Instance not found"),
    })?;
    Ok(Json(instance.configurable_manifest().await))
}

pub async fn set_instance_setting(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, section_id, setting_id)): Path<(InstanceUuid, String, String)>,
    AuthBearer(token): AuthBearer,
    Json(value): Json<ConfigurableValue>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    let instance = state.instances.get(&uuid).ok_or(Error {
        kind: ErrorKind::NotFound,
        source: eyre!("Instance not found"),
    })?;

    instance
        .update_configurable(&section_id, &setting_id, value)
        .await?;

    Ok(Json(()))
}

pub async fn set_instance_name(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(new_name): Json<String>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .set_name(new_name)
        .await?;
    Ok(Json(()))
}

pub async fn set_instance_description(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(new_description): Json<String>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .set_description(new_description)
        .await?;
    Ok(Json(()))
}

pub async fn set_instance_folder(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(folder): Json<String>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    if uuid.to_string().starts_with("DOCKER-") {
        return Err(Error {
            kind: ErrorKind::UnsupportedOperation,
            source: eyre!("Docker instances do not have a folder"),
        });
    }
    let path = {
        let instance = state.instances.get(&uuid).ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?;
        let path = instance.path().await;
        drop(instance);
        path
    };
    crate::instance_folder::write_instance_folder(&path, &folder).await?;
    Ok(Json(()))
}

pub async fn set_instance_auto_start(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(auto_start): Json<bool>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .set_auto_start(auto_start)
        .await?;
    Ok(Json(()))
}

pub async fn set_instance_restart_on_crash(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(restart_on_crash): Json<bool>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .set_restart_on_crash(restart_on_crash)
        .await?;
    Ok(Json(()))
}

pub async fn change_version(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, new_version)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    if uuid.to_string().starts_with("DOCKER-") {
        return Err(Error {
            kind: ErrorKind::UnsupportedOperation,
            source: eyre!("Docker instances have no server jar to update"),
        });
    }
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    instance.change_version(new_version).await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct LoaderUpdateRequest {
    pub version: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub installer_version: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
}

pub async fn update_loader(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(body): Json<LoaderUpdateRequest>,
) -> Result<Json<()>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    requester.try_action(
        &UserAction::AccessSetting(uuid.clone()),
        state.global_settings.lock().await.safe_mode(),
    )?;
    if uuid.to_string().starts_with("DOCKER-") {
        return Err(Error {
            kind: ErrorKind::UnsupportedOperation,
            source: eyre!("Docker instances have no server jar to update"),
        });
    }
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    match instance {
        GameInstance::MinecraftInstance(instance) => {
            instance
                .update_loader(LoaderUpdate {
                    version: body.version,
                    loader_version: body.loader_version,
                    installer_version: body.installer_version,
                    build: body.build,
                })
                .await?;
        }
        GameInstance::GenericInstance(_) => {
            return Err(Error {
                kind: ErrorKind::UnsupportedOperation,
                source: eyre!("This instance has no server jar to update"),
            })
        }
    }
    Ok(Json(()))
}

pub fn get_instance_config_routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/instance/:uuid/configurable_manifest",
            get(get_instance_configurable_manifest),
        )
        .route("/instance/:uuid/version/:new_version", put(change_version))
        .route("/instance/:uuid/loader", put(update_loader))
        .route("/instance/:uuid/settings", get(get_instance_settings))
        .route(
            "/instance/:uuid/settings/:section_id/:setting_id",
            put(set_instance_setting),
        )
        .route("/instance/:uuid/name", put(set_instance_name))
        .route("/instance/:uuid/description", put(set_instance_description))
        .route("/instance/:uuid/folder", put(set_instance_folder))
        .route("/instance/:uuid/auto_start", put(set_instance_auto_start))
        .route(
            "/instance/:uuid/restart_on_crash",
            put(set_instance_restart_on_crash),
        )
        .with_state(state)
}
