mod stats;
mod virtual_fs;
pub use virtual_fs::is_container_mount_root;

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bollard::container::{
    AttachContainerOptions, AttachContainerResults, ListContainersOptions, MemoryStatsStats,
    StatsOptions,
};
use bollard::{secret::EventMessage, system::EventsOptions, Docker};
use color_eyre::eyre::{eyre, Context, ContextCompat};

use tokio::sync::{Mutex, RwLock};
use tokio_stream::StreamExt;
use tracing::{error, info};
use virtual_fs::{get_virtual_path, to_virtual_path};

use crate::handlers::global_fs::FileEntry;
use crate::traits::t_configurable::Game::Generic;
use crate::util::{list_dir, scoped_join_win_safe};
use crate::{
    error::{Error, ErrorKind},
    event_broadcaster::EventBroadcaster,
    events::Event,
    traits::{
        t_configurable::GameType,
        t_server::{MonitorReport, State},
        InstanceInfo,
    },
    types::InstanceUuid,
};

use stats::{docker_cpu_percent, docker_memory_bytes};

#[derive(Debug, Clone)]
pub struct DockerBridge {
    docker: Docker,
    event_broadcaster: EventBroadcaster,
    watch_list: Arc<RwLock<HashSet<InstanceUuid>>>,
    db_file_path: PathBuf,
    /// Previous CPU counters, so a one-shot stats read can show a real delta.
    cpu_prev: Arc<Mutex<HashMap<InstanceUuid, (u64, u64)>>>,
}

fn docker_id_to_uuid(name: &str) -> InstanceUuid {
    format!("DOCKER-{}", name).into()
}

fn extract_container_id(event: &EventMessage) -> Option<String> {
    event.actor.as_ref().and_then(|actor| actor.id.clone())
}

fn extract_container_name(event: &EventMessage) -> Option<String> {
    event.actor.as_ref().and_then(|actor| {
        actor
            .attributes
            .as_ref()
            .and_then(|attr| attr.get("name").map(|name| name.to_string()))
    })
}

fn docker_event_to_lodestone_event(event: EventMessage) -> Option<Event> {
    let instance_uuid = docker_id_to_uuid(&extract_container_name(&event)?);

    let instance_name = extract_container_name(&event)?;
    Some(match event.action?.as_str() {
        "start" => {
            Event::new_instance_state_transition(instance_uuid, instance_name, State::Running)
        }
        "stop" => {
            Event::new_instance_state_transition(instance_uuid, instance_name, State::Stopping)
        }
        "die" => Event::new_instance_state_transition(instance_uuid, instance_name, State::Stopped),
        _ => return None,
    })
}

impl DockerBridge {
    pub async fn new(
        event_broadcaster: EventBroadcaster,
        db_file_path: PathBuf,
    ) -> Result<Self, Error> {
        let docker =
            Docker::connect_with_local_defaults().context("Failed to connect to docker")?;
        // make sure the db file exists
        let file = File::options()
            .create(true)
            .write(true)
            .read(true)
            .open(&db_file_path)
            .context("Failed to open db file")?;
        let watch_list: HashSet<InstanceUuid> =
            if let Ok(watch_list) = serde_json::from_reader(file) {
                watch_list
            } else {
                info!("Creating new docker bridge db file");
                // if the file is empty, create an empty watch list and write it to the file
                let watch_list = HashSet::new();
                let file = File::options()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&db_file_path)
                    .context("Failed to open db file")?;
                serde_json::to_writer(file, &watch_list).context("Failed to write db file")?;
                watch_list
            };

        let watch_list = Arc::new(RwLock::new(watch_list));
        tokio::spawn({
            let docker = docker.clone();
            let event_broadcaster = event_broadcaster.clone();
            let watch_list = watch_list.clone();
            async move {
                let mut stream = docker.events(None::<EventsOptions<String>>);
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(event) => {
                            if !watch_list.read().await.contains(&docker_id_to_uuid(
                                &extract_container_name(&event).unwrap(),
                            )) {
                                continue;
                            }
                            if let Some(lodestone_event) = docker_event_to_lodestone_event(event) {
                                event_broadcaster.send(lodestone_event);
                            }
                        }
                        Err(e) => {
                            error!("Error while listening to docker events: {:?}", e);
                        }
                    }
                }
            }
        });
        Ok(Self {
            docker,
            db_file_path,
            event_broadcaster,
            watch_list,
            cpu_prev: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn watched_uuids(&self) -> Vec<InstanceUuid> {
        self.watch_list.read().await.iter().cloned().collect()
    }

    /// Latest CPU and memory sample. The first call has no CPU delta yet.
    pub async fn sample(&self, uuid: &InstanceUuid) -> Result<MonitorReport, Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        let mut stream = self.docker.stats(
            &name,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        let stats = stream
            .next()
            .await
            .ok_or_else(|| Error {
                kind: ErrorKind::NotFound,
                source: eyre!("Docker returned no stats"),
            })?
            .map_err(|err| Error {
                kind: ErrorKind::External,
                source: err.into(),
            })?;
        let inactive = match stats.memory_stats.stats {
            Some(MemoryStatsStats::V1(stats)) => stats.total_inactive_file,
            Some(MemoryStatsStats::V2(stats)) => stats.inactive_file,
            None => 0,
        };
        let cpu_total = stats.cpu_stats.cpu_usage.total_usage;
        let system_total = stats.cpu_stats.system_cpu_usage.unwrap_or(0);
        let cpu_usage = {
            let mut previous = self.cpu_prev.lock().await;
            let usage = previous.get(uuid).and_then(|(prev_cpu, prev_system)| {
                docker_cpu_percent(cpu_total, *prev_cpu, system_total, *prev_system)
            });
            previous.insert(uuid.clone(), (cpu_total, system_total));
            usage
        };
        Ok(MonitorReport {
            memory_usage: docker_memory_bytes(stats.memory_stats.usage, inactive),
            disk_usage: None,
            cpu_usage,
            start_time: None,
        })
    }

    pub async fn list_containers(&self) -> Result<Vec<InstanceInfo>, Error> {
        let mut ret = Vec::new();
        let containers = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await
            .context("Failed to list containers")?;
        for container in containers {
            let name = container.names.unwrap()[0].clone().replace('/', "");
            let uuid = docker_id_to_uuid(&name);
            if !self.watch_list.read().await.contains(&uuid) {
                continue;
            }
            let instance = InstanceInfo {
                uuid,
                name,
                game_type: Generic {
                    game_name: GameType::Generic,
                    game_display_name: "Docker".to_string(),
                },
                description: "Docker container test".to_string(),
                version: "".to_string(),
                port: 0,
                creation_time: container.created.unwrap_or(0),
                path: "/no_peek/Volume".to_string(),
                auto_start: false,
                restart_on_crash: false,
                state: State::from_docker_state_string(&container.state.unwrap()),
                player_count: None,
                max_player_count: None,
                player_list: None,
                folder: String::new(),
            };
            ret.push(instance);
        }
        Ok(ret)
    }

    pub async fn get_container_state(&self, uuid: &InstanceUuid) -> Result<State, Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        let state = self
            .docker
            .inspect_container(&name, None)
            .await
            .context("Failed to inspect container")?
            .state
            .context("Failed to get container state")?;
        Ok(State::from_container_state(&state))
    }

    pub async fn stop_container(&self, uuid: &InstanceUuid) -> Result<(), Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        self.docker
            .stop_container(&name, None::<bollard::container::StopContainerOptions>)
            .await
            .context("Failed to stop container")?;
        Ok(())
    }

    pub async fn start_container(&self, uuid: &InstanceUuid) -> Result<(), Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        self.docker
            .start_container(
                &name,
                None::<bollard::container::StartContainerOptions<String>>,
            )
            .await
            .context("Failed to start container")?;
        let options = Some(AttachContainerOptions::<String> {
            stdin: Some(true),
            stdout: Some(true),
            stderr: Some(true),
            stream: Some(true),
            logs: Some(false),
            detach_keys: None,
        });
        let AttachContainerResults { mut output, .. } =
            self.docker.attach_container(&name, options).await.unwrap();

        let event_broadcaster = self.event_broadcaster.clone();
        tokio::spawn({
            let name = name.clone();
            let uuid = uuid.clone();
            async move {
                while let Some(Ok(output)) = output.next().await {
                    let name = name.clone();
                    let uuid = uuid.clone();
                    event_broadcaster.send(Event::new_instance_output(
                        uuid.clone(),
                        name,
                        output.to_string(),
                    ));
                    output.to_string();
                }
            }
        });

        Ok(())
    }

    pub async fn restart_container(&self, uuid: &InstanceUuid) -> Result<(), Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        self.docker
            .restart_container(&name, None::<bollard::container::RestartContainerOptions>)
            .await
            .context("Failed to restart container")?;
        Ok(())
    }

    pub async fn kill_container(&self, uuid: &InstanceUuid) -> Result<(), Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        self.docker
            .kill_container(
                &name,
                None::<bollard::container::KillContainerOptions<String>>,
            )
            .await
            .context("Failed to kill container")?;
        Ok(())
    }

    async fn sync_watch_list(&self) -> Result<(), Error> {
        let watch_list = self.watch_list.read().await;
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.db_file_path)
            .context("Failed to open db file")?;
        serde_json::to_writer(file, &*watch_list).context("Failed to write db file")?;
        Ok(())
    }

    pub async fn add_to_watch_list(&self, name: String) {
        let mut watch_list = self.watch_list.write().await;
        watch_list.insert(docker_id_to_uuid(&name));
        drop(watch_list);
        let _ = self.sync_watch_list().await.map_err(|e| {
            error!("Failed to add to watch list: {:?}", e);
        });
    }

    pub async fn remove_from_watch_list(&self, name: String) {
        let mut watch_list = self.watch_list.write().await;
        watch_list.remove(&docker_id_to_uuid(&name));
        drop(watch_list);
        let _ = self.sync_watch_list().await.map_err(|e| {
            error!("Failed to remove from watch list: {:?}", e);
        });
    }

    /// Host path for a file inside one of the container's mounts.
    /// `relative_path` starts with the mount name, such as `data/world/level.dat`.
    pub async fn host_path(
        &self,
        uuid: &InstanceUuid,
        relative_path: &Path,
    ) -> Result<PathBuf, Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        let inspect = self
            .docker
            .inspect_container(&name, None)
            .await
            .context("Failed to inspect container")?;
        let virtual_roots: Vec<PathBuf> = inspect
            .mounts
            .unwrap_or_default()
            .iter()
            .filter_map(|mount| mount.source.clone())
            .map(PathBuf::from)
            .collect();
        if virtual_roots.is_empty() {
            return Err(Error {
                kind: ErrorKind::NotFound,
                source: eyre!("Container has no mounted directories"),
            });
        }
        get_virtual_path(&virtual_roots, relative_path)
            .map(|(path, _, _)| path)
            .ok_or_else(|| Error {
                kind: ErrorKind::BadRequest,
                source: eyre!("Path is outside the container mounts"),
            })
    }

    pub async fn list_files(
        &self,
        uuid: &InstanceUuid,
        relative_path: PathBuf,
    ) -> Result<Vec<FileEntry>, Error> {
        let name = uuid.to_string().replace("DOCKER-", "");
        let mounts = &self
            .docker
            .inspect_container(&name, None)
            .await
            .context("Failed to inspect container")?
            .mounts;
        if mounts.is_none() {
            return Ok(Vec::new());
        }
        let mounts = mounts.as_ref().unwrap();
        if mounts.is_empty() {
            return Ok(Vec::new());
        }
        let safe_relative_path = scoped_join_win_safe("/", &relative_path)?;
        if safe_relative_path == PathBuf::from("/") {
            return Ok(mounts
                .iter()
                .filter_map(|m| {
                    let path = PathBuf::from(m.clone().source?);
                    let mut r: FileEntry = path.as_path().into();
                    r.path = path.components().last()?.as_os_str().to_str()?.to_owned();
                    Some(r)
                })
                .collect());
        }
        let virtual_roots: Vec<PathBuf> = mounts
            .iter()
            .filter_map(|m| m.source.as_ref())
            .map(PathBuf::from)
            .collect();
        let (path, v_root, mount_point) = get_virtual_path(&virtual_roots, &relative_path).unwrap();
        // dbg!(&path, &mount_point);
        let ret: Vec<FileEntry> = list_dir(&path, None)
            .await?
            .iter()
            .map(move |p| -> FileEntry {
                // remove the root path from the file path
                let mut r: FileEntry = p.as_path().into();
                r.path = to_virtual_path(p, &v_root, &mount_point)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                r
            })
            .collect();

        Ok(ret)
    }

    pub async fn read_container_file(
        &self,
        uuid: &InstanceUuid,
        relative_path: PathBuf,
    ) -> Result<String, Error> {
        let path = self.host_path(uuid, &relative_path).await?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .context("Failed to read file")?;
        Ok(content)
    }

    pub async fn write_container_file(
        &self,
        uuid: &InstanceUuid,
        relative_path: PathBuf,
        content: &[u8],
    ) -> Result<(), Error> {
        let path = self.host_path(uuid, &relative_path).await?;
        tokio::fs::write(&path, content)
            .await
            .context("Failed to write file")?;
        Ok(())
    }
}
