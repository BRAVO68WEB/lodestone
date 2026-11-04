use std::sync::OnceLock;

use axum::body::StreamBody;
use axum::extract::Path;
use axum::http::{header, HeaderName, HeaderValue};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;
use tokio_util::io::ReaderStream;

use crate::auth::user::UserAction;
use crate::error::{Error, ErrorKind};
use crate::implementations::minecraft::MinecraftInstance;
use crate::instance_backup::{
    begin_instance_backup, delete_instance_backup, list_instance_backups, resolve_backup,
    InstanceBackup,
};
use crate::prelude::GameInstance;
use crate::traits::t_configurable::{Game, TConfigurable};
use crate::traits::t_server::{State, TServer};
use crate::types::{DotLodestoneConfig, InstanceUuid};
use crate::AppState;

fn backup_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn docker_unsupported() -> Error {
    Error {
        kind: ErrorKind::UnsupportedOperation,
        source: eyre!("Docker instances cannot be backed up here"),
    }
}

fn not_found() -> Error {
    Error {
        kind: ErrorKind::NotFound,
        source: eyre!("Instance not found"),
    }
}

fn stopped_error(action: &str) -> Error {
    Error {
        kind: ErrorKind::BadRequest,
        source: eyre!("Stop the server before {action}"),
    }
}

async fn authorize(
    state: &AppState,
    token: &str,
    action: &UserAction,
) -> Result<(), Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(token)?;
    requester.try_action(action, state.global_settings.lock().await.safe_mode())?;
    Ok(())
}

fn open_instance(state: &AppState, uuid: &InstanceUuid) -> Result<GameInstance, Error> {
    if uuid.to_string().starts_with("DOCKER-") {
        return Err(docker_unsupported());
    }
    state
        .instances
        .get(uuid)
        .map(|instance| instance.clone())
        .ok_or_else(not_found)
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    match tokio::task::spawn_blocking(job).await {
        Ok(result) => result,
        Err(err) => Err(Error {
            kind: ErrorKind::Internal,
            source: eyre!("Backup task stopped: {err}"),
        }),
    }
}

pub async fn list_backups(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<Vec<InstanceBackup>>, Error> {
    authorize(
        &state,
        &token,
        &UserAction::ReadInstanceFile(uuid.clone()),
    )
    .await?;
    let instance = open_instance(&state, &uuid)?;
    let path = instance.path().await;
    Ok(Json(blocking(move || list_instance_backups(&path)).await?))
}

pub async fn create_backup(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<InstanceBackup>, Error> {
    authorize(
        &state,
        &token,
        &UserAction::WriteInstanceFile(uuid.clone()),
    )
    .await?;
    let instance = open_instance(&state, &uuid)?;
    let _guard = backup_lock().lock().await;
    crate::instance_backup::require_stopped(instance.state().await, "taking a backup")?;
    let path = instance.path().await;
    let now = chrono::Utc::now();
    let partial = blocking(move || begin_instance_backup(&path, now)).await?;
    if instance.state().await != State::Stopped {
        partial.discard();
        return Err(stopped_error("taking a backup"));
    }
    Ok(Json(blocking(move || partial.finish()).await?))
}

pub async fn download_backup(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((uuid, name)): Path<(InstanceUuid, String)>,
    AuthBearer(token): AuthBearer,
) -> Result<
    (
        [(HeaderName, HeaderValue); 3],
        StreamBody<ReaderStream<tokio::fs::File>>,
    ),
    Error,
> {
    authorize(
        &state,
        &token,
        &UserAction::ReadInstanceFile(uuid.clone()),
    )
    .await?;
    let instance = open_instance(&state, &uuid)?;
    let path = instance.path().await;
    let archive = resolve_backup(&path, &name)?;
    let file = tokio::fs::File::open(&archive)
        .await
        .map_err(|_| not_found())?;
    let length = file
        .metadata()
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/zip"),
        ),
        (
            header::CONTENT_DISPOSITION,
            header_value(&format!("attachment; filename=\"{name}\""))?,
        ),
        (
            header::CONTENT_LENGTH,
            header_value(&length.to_string())?,
        ),
    ];
    Ok((headers, StreamBody::new(ReaderStream::new(file))))
}

pub async fn remove_backup(
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
    let _guard = backup_lock().lock().await;
    let path = instance.path().await;
    blocking(move || delete_instance_backup(&path, &name)).await?;
    Ok(Json(()))
}

pub async fn restore_backup(
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
    let _guard = backup_lock().lock().await;
    crate::instance_backup::require_stopped(instance.state().await, "restoring a backup")?;
    let path = instance.path().await;
    let archive = resolve_backup(&path, &name)?;
    let path_for_restore = path.clone();
    let prepared = blocking(move || {
        crate::instance_backup::prepare_restore(&path_for_restore, &archive)
    })
    .await?;
    if instance.state().await != State::Stopped {
        prepared.rollback()?;
        return Err(stopped_error("restoring a backup"));
    }
    if let Err(err) = reload_restored(&state, &uuid, &instance, path).await {
        if let Err(rollback_err) = prepared.rollback() {
            return Err(Error {
                kind: ErrorKind::Internal,
                source: eyre!(
                    "Restore failed ({err}) and could not be undone ({rollback_err})"
                ),
            });
        }
        return Err(err);
    }
    prepared.commit();
    Ok(Json(()))
}

async fn reload_restored(
    state: &AppState,
    uuid: &InstanceUuid,
    instance: &GameInstance,
    path: std::path::PathBuf,
) -> Result<(), Error> {
    match instance.game_type().await {
        Game::MinecraftJava { .. } => {
            let previous_port = instance.port().await;
            let config_path = path.join(".lodestone_config");
            let config: DotLodestoneConfig = serde_json::from_reader(
                std::fs::File::open(&config_path).map_err(|_| {
                    Error {
                        kind: ErrorKind::Internal,
                        source: eyre!("Restored instance is missing .lodestone_config"),
                    }
                })?,
            )
            .map_err(|_| Error {
                kind: ErrorKind::Internal,
                source: eyre!("Restored instance id could not be read"),
            })?;
            if config.uuid() != uuid {
                return Err(Error {
                    kind: ErrorKind::Internal,
                    source: eyre!("Restored instance id does not match"),
                });
            }
            let reloaded = MinecraftInstance::restore(
                path,
                config,
                state.event_broadcaster.clone(),
                state.macro_executor.clone(),
            )
            .await?;
            let new_port = reloaded.port().await;
            state.instances.insert(uuid.clone(), reloaded.into());
            if previous_port != new_port {
                let mut ports = state.port_manager.lock().await;
                ports.deallocate(previous_port);
                ports.add_port(new_port);
            }
            Ok(())
        }
        Game::Generic { .. } => Ok(()),
        Game::MinecraftBedrock => Err(Error {
            kind: ErrorKind::UnsupportedOperation,
            source: eyre!("This instance cannot be restored"),
        }),
    }
}

fn header_value(value: &str) -> Result<HeaderValue, Error> {
    HeaderValue::from_str(value).map_err(|err| Error {
        kind: ErrorKind::Internal,
        source: eyre!("Invalid header: {err}"),
    })
}

pub fn get_instance_backup_routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/instance/:uuid/backups",
            get(list_backups).post(create_backup),
        )
        .route(
            "/instance/:uuid/backups/:name/restore",
            post(restore_backup),
        )
        .route(
            "/instance/:uuid/backups/:name",
            get(download_backup).delete(remove_backup),
        )
        .with_state(state)
}
