use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{ws::WebSocket, Path, WebSocketUpgrade},
    response::Response,
    routing::get,
    Json, Router,
};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;
use futures::{SinkExt, StreamExt};
use ringbuffer::{AllocRingBuffer, RingBufferExt};
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::error;

use crate::{
    auth::user::UserAction,
    error::Error,
    prelude::GameInstance,
    traits::{t_server::MonitorReport, t_server::TServer},
    types::InstanceUuid,
    AppState,
};

pub async fn monitor(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(uuid): Path<InstanceUuid>,
) -> Result<Response, Error> {
    let buffer = state.monitor_buffer.clone();
    if uuid.to_string().starts_with("DOCKER-") {
        return Ok(ws.on_upgrade(move |stream| monitor_docker_ws(stream, buffer, uuid)));
    }
    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: crate::error::ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .to_owned();
    Ok(ws.on_upgrade(move |stream| monitor_ws(stream, buffer, instance, uuid)))
}

async fn monitor_docker_ws(
    stream: WebSocket,
    monitor_buffer: Arc<Mutex<HashMap<InstanceUuid, AllocRingBuffer<MonitorReport>>>>,
    uuid: InstanceUuid,
) {
    let (mut tx, mut rx) = stream.split();
    if let Some(buffer) = monitor_buffer.lock().await.get(&uuid) {
        for report in buffer.iter() {
            if tx
                .send(axum::extract::ws::Message::Text(
                    serde_json::to_string(&report).unwrap(),
                ))
                .await
                .is_err()
            {
                return;
            }
        }
    }
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let report = monitor_buffer
                    .lock()
                    .await
                    .get(&uuid)
                    .and_then(|ring| ring.iter().last().cloned())
                    .unwrap_or_default();
                if tx
                    .send(axum::extract::ws::Message::Text(
                        serde_json::to_string(&report).unwrap(),
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            msg = rx.next() => {
                if msg.is_none() {
                    break;
                }
            }
        }
    }
}

async fn monitor_ws(
    stream: WebSocket,
    monitor_buffer: Arc<Mutex<HashMap<InstanceUuid, AllocRingBuffer<MonitorReport>>>>,
    instance: GameInstance,
    uuid: InstanceUuid,
) {
    let (mut tx, mut rx) = stream.split();
    if let Some(buffer) = monitor_buffer.lock().await.get(&uuid) {
        for report in buffer.iter() {
            if let Err(e) = tx
                .send(axum::extract::ws::Message::Text(
                    serde_json::to_string(&report).unwrap(),
                ))
                .await
            {
                error!("1 Error sending monitor report: {}", e);
                break;
            }
        }
    }
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let monitor = instance.monitor().await;
                if let Err(e) = tx
                    .send(axum::extract::ws::Message::Text(
                        serde_json::to_string(&monitor).unwrap(),
                    ))
                    .await
                {
                    error!("2 Error sending monitor report: {}", e);
                    break;
                }
            }
            msg = rx.next() => {
                if msg.is_none() {
                    break;
                }
            }
        }
    }
}

#[derive(Serialize)]
pub struct MonitorSample {
    pub uuid: InstanceUuid,
    pub cpu_usage: Option<f32>,
    pub memory_usage: Option<u64>,
}

/// Latest stored sample for each instance the caller is allowed to view.
pub async fn monitor_latest(
    axum::extract::State(state): axum::extract::State<AppState>,
    AuthBearer(token): AuthBearer,
) -> Result<Json<Vec<MonitorSample>>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    let docker_ids = state.docker_bridge.watched_uuids().await;
    let buffer = state.monitor_buffer.lock().await;
    let mut samples = Vec::new();
    for instance in state.instances.iter() {
        let uuid = instance.key().clone();
        if !requester.can_perform_action(&UserAction::ViewInstance(uuid.clone())) {
            continue;
        }
        let report = buffer.get(&uuid).and_then(|ring| ring.iter().last().cloned());
        samples.push(MonitorSample {
            uuid,
            cpu_usage: report.as_ref().and_then(|item| item.cpu_usage),
            memory_usage: report.as_ref().and_then(|item| item.memory_usage),
        });
    }
    for uuid in docker_ids {
        if samples.iter().any(|sample| sample.uuid == uuid) {
            continue;
        }
        if !requester.can_perform_action(&UserAction::ViewInstance(uuid.clone())) {
            continue;
        }
        let report = buffer.get(&uuid).and_then(|ring| ring.iter().last().cloned());
        samples.push(MonitorSample {
            uuid,
            cpu_usage: report.as_ref().and_then(|item| item.cpu_usage),
            memory_usage: report.as_ref().and_then(|item| item.memory_usage),
        });
    }
    Ok(Json(samples))
}

pub fn get_monitor_routes(state: AppState) -> Router {
    Router::new()
        .route("/monitor/latest", get(monitor_latest))
        .route("/monitor/:uuid", get(monitor))
        .with_state(state)
}
