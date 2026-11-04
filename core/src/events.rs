#![allow(clippy::enum_variant_names)]

use std::{collections::HashSet, path::PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    auth::{permission::UserPermission, user_id::UserId},
    macro_executor::MacroPID,
    output_types::ClientEvent,
    traits::{t_macro::ExitStatus, t_player::Player, t_server::State, InstanceInfo},
    types::{InstanceUuid, Snowflake, TimeRange},
};

pub trait EventFilter {
    fn filter(&mut self, event: impl AsRef<ClientEvent>) -> bool;
}

#[derive(Deserialize, Clone, Debug, TS)]
#[ts(export)]
pub struct EventQuery {
    pub event_levels: Option<Vec<EventLevel>>,
    pub event_types: Option<Vec<EventType>>,
    pub instance_event_types: Option<Vec<InstanceEventKind>>,
    pub user_event_types: Option<Vec<UserEventKind>>,
    pub event_user_ids: Option<Vec<UserId>>,
    pub event_instance_ids: Option<Vec<InstanceUuid>>,
    pub bearer_token: Option<String>,
    pub time_range: Option<TimeRange>,
    /// Drop console lines. The console buffer is the place for those.
    #[serde(default)]
    pub exclude_console: bool,
}

impl EventQuery {
    pub fn filter(&self, event: impl AsRef<ClientEvent>) -> bool {
        let event = event.as_ref();
        if let Some(event_levels) = &self.event_levels {
            if !event_levels.contains(&event.level) {
                return false;
            }
        }
        if let Some(event_types) = &self.event_types {
            if !event_types.contains(&event.event_inner.as_ref().into()) {
                return false;
            }
        }
        if let Some(instance_event_types) = &self.instance_event_types {
            if let EventInner::InstanceEvent(instance_event) = &event.event_inner {
                if !instance_event_types
                    .contains(&instance_event.instance_event_inner.as_ref().into())
                {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(user_event_types) = &self.user_event_types {
            if let EventInner::UserEvent(user_event) = &event.event_inner {
                if !user_event_types.contains(&user_event.user_event_inner.as_ref().into()) {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(event_user_ids) = &self.event_user_ids {
            if let EventInner::UserEvent(user_event) = &event.event_inner {
                if !event_user_ids.contains(&user_event.user_id) {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(event_instance_ids) = &self.event_instance_ids {
            match event_instance_uuid(&event.event_inner) {
                Some(instance_uuid) if event_instance_ids.contains(&instance_uuid) => {}
                _ => return false,
            }
        }
        if self.exclude_console && event.event_inner.is_console_message() {
            return false;
        }
        // TODO might need to check time too
        true
    }
}

/// Instance this event belongs to, including progression and macro events.
pub fn event_instance_uuid(inner: &EventInner) -> Option<InstanceUuid> {
    match inner {
        EventInner::InstanceEvent(instance_event) => Some(instance_event.instance_uuid.clone()),
        EventInner::MacroEvent(macro_event) => macro_event.instance_uuid.clone(),
        EventInner::ProgressionEvent(progression) => match progression.progression_event_inner() {
            ProgressionEventInner::ProgressionStart { inner, .. } => match inner {
                Some(ProgressionStartValue::InstanceCreation { instance_uuid })
                | Some(ProgressionStartValue::InstanceDelete { instance_uuid }) => {
                    Some(instance_uuid.clone())
                }
                None => None,
            },
            ProgressionEventInner::ProgressionUpdate { .. } => None,
            ProgressionEventInner::ProgressionEnd { inner, .. } => match inner {
                Some(ProgressionEndValue::InstanceCreation(info)) => Some(info.uuid.clone()),
                Some(ProgressionEndValue::InstanceDelete { instance_uuid })
                | Some(ProgressionEndValue::FSOperationCompleted { instance_uuid, .. }) => {
                    Some(instance_uuid.clone())
                }
                None => None,
            },
        },
        EventInner::UserEvent(_) | EventInner::FSEvent(_) | EventInner::PlayitggRunnerEvent(_) => {
            None
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
#[derive(enum_kinds::EnumKind)]
#[enum_kind(InstanceEventKind, derive(Serialize, Deserialize, TS))]
pub enum InstanceEventInner {
    StateTransition {
        to: State,
    },
    InstanceWarning {
        message: String,
    },
    InstanceError {
        message: String,
    },
    InstanceInput {
        message: String,
    },
    InstanceOutput {
        message: String,
    },
    SystemMessage {
        message: String,
    },
    PlayerChange {
        player_list: HashSet<Player>,
        players_joined: HashSet<Player>,
        players_left: HashSet<Player>,
    },

    PlayerMessage {
        player: String,
        player_message: String,
    },
}

impl AsRef<InstanceEventInner> for InstanceEventInner {
    fn as_ref(&self) -> &InstanceEventInner {
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct InstanceEvent {
    pub instance_uuid: InstanceUuid,
    pub instance_name: String,
    pub instance_event_inner: InstanceEventInner,
}
#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
#[derive(enum_kinds::EnumKind)]
#[enum_kind(UserEventKind, derive(Serialize, Deserialize, TS))]
pub enum UserEventInner {
    UserCreated,
    UserDeleted,
    UserLoggedIn,
    UserLoggedOut,
    UsernameChanged {
        new_username: String,
    },
    PermissionChanged {
        new_permissions: Box<UserPermission>,
    },
}

impl AsRef<UserEventInner> for UserEventInner {
    fn as_ref(&self) -> &UserEventInner {
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct UserEvent {
    pub user_id: UserId,
    pub user_event_inner: UserEventInner,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum MacroEventInner {
    Started,
    /// Macro requests to be detached, useful for macros that run in the background such as prelaunch script
    Detach,
    Stopped {
        exit_status: ExitStatus,
    },
}
#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct MacroEvent {
    pub instance_uuid: Option<InstanceUuid>,
    pub macro_pid: MacroPID,
    /// Display name of the macro. Empty for internal workers such as a generic instance runtime.
    #[serde(default)]
    pub task_name: String,
    /// Unix seconds when this run was created. `0` when the emitter has no task row.
    #[serde(default)]
    pub creation_time: i64,
    pub macro_event_inner: MacroEventInner,
}

impl MacroEvent {
    pub fn new(
        instance_uuid: Option<InstanceUuid>,
        macro_pid: MacroPID,
        task_name: impl Into<String>,
        creation_time: i64,
        macro_event_inner: MacroEventInner,
    ) -> Self {
        Self {
            instance_uuid,
            macro_pid,
            task_name: task_name.into(),
            creation_time,
            macro_event_inner,
        }
    }
}

impl From<MacroEvent> for Event {
    fn from(val: MacroEvent) -> Self {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::MacroEvent(val.clone()),
            caused_by: CausedBy::Macro {
                macro_pid: val.macro_pid,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum ProgressionEndValue {
    InstanceCreation(InstanceInfo),
    InstanceDelete {
        instance_uuid: InstanceUuid,
    },
    FSOperationCompleted {
        instance_uuid: InstanceUuid,
        success: bool,
        message: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum ProgressionStartValue {
    InstanceCreation { instance_uuid: InstanceUuid },
    InstanceDelete { instance_uuid: InstanceUuid },
}

// the backend will keep exactly 1 copy of ProgressionStart, and 1 copy of ProgressionUpdate OR ProgressionEnd
#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum ProgressionEventInner {
    ProgressionStart {
        progression_name: String,
        total: Option<f64>,
        inner: Option<ProgressionStartValue>,
    },
    ProgressionUpdate {
        progress_message: String,
        progress: f64,
    },
    ProgressionEnd {
        success: bool,
        message: Option<String>,
        inner: Option<ProgressionEndValue>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum PlayitggRunnerEventInner {
    RunnerStarted,
    RunnerLoading,
    RunnerStopped,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub enum FSOperation {
    Read,
    Write,
    Move { source: PathBuf },
    Create,
    Delete,
    Upload,
    Download,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[serde(tag = "type", content = "path")]
#[ts(export)]
pub enum FSTarget {
    File(PathBuf),
    Directory(PathBuf),
}
#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct FSEvent {
    pub operation: FSOperation,
    pub target: FSTarget,
}

pub fn new_fs_event(operation: FSOperation, target: FSTarget, caused_by: CausedBy) -> Event {
    Event {
        details: "".to_string(),
        snowflake: Snowflake::default(),
        event_inner: EventInner::FSEvent(FSEvent { operation, target }),
        caused_by,
    }
}

#[derive(Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export)]
pub struct ProgressionEventID(Snowflake);

impl ProgressionEventID {
    pub fn inner(&self) -> Snowflake {
        self.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct ProgressionEvent {
    event_id: Snowflake,
    progression_event_inner: ProgressionEventInner,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
pub struct PlayitggRunnerEvent {
    pub playitgg_runner_event_inner: PlayitggRunnerEventInner,
}

impl ProgressionEvent {
    pub fn event_id(&self) -> Snowflake {
        self.event_id
    }
    pub fn progression_event_inner(&self) -> &ProgressionEventInner {
        &self.progression_event_inner
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
#[derive(enum_kinds::EnumKind)]
#[enum_kind(EventType, derive(Serialize, Deserialize, TS))]
pub enum EventInner {
    InstanceEvent(InstanceEvent),
    UserEvent(UserEvent),
    MacroEvent(MacroEvent),
    FSEvent(FSEvent),
    ProgressionEvent(ProgressionEvent),
    PlayitggRunnerEvent(PlayitggRunnerEvent),
}

impl AsRef<EventInner> for EventInner {
    fn as_ref(&self) -> &EventInner {
        self
    }
}

impl EventInner {
    pub fn is_console_message(&self) -> bool {
        match self {
            EventInner::InstanceEvent(instance_event) => matches!(
                &instance_event.instance_event_inner,
                InstanceEventInner::InstanceOutput { .. }
                    | InstanceEventInner::PlayerMessage { .. }
                    | InstanceEventInner::SystemMessage { .. }
            ),
            _ => false,
        }
    }
}

#[test]
fn event_type_export() {
    let _ = EventType::export();
    let _ = UserEventKind::export();
    let _ = InstanceEventKind::export();
}
#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[ts(export)]
#[serde(tag = "type")]
pub enum CausedBy {
    User { user_id: UserId, user_name: String },
    Instance { instance_uuid: InstanceUuid },
    Macro { macro_pid: MacroPID },
    System,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq)]
#[serde(into = "ClientEvent")]
pub struct Event {
    pub event_inner: EventInner,
    pub details: String,
    pub snowflake: Snowflake,
    pub caused_by: CausedBy,
}

pub trait IntoEvent {
    fn into_event(self, caused_by: CausedBy, details: String) -> Event;
}

#[derive(Serialize, Deserialize, Clone, Debug, TS, PartialEq, Eq)]
#[ts(export)]
#[derive(sqlx::Type)]
pub enum EventLevel {
    Info,
    Warning,
    Error,
}

// impl From<&EventInner> for EventType {
//     fn from(event_inner: &EventInner) -> Self {
//         match event_inner {
//             EventInner::InstanceEvent(_) => EventType::InstanceEvent,
//             EventInner::UserEvent(_) => EventType::UserEvent,
//             EventInner::MacroEvent(_) => EventType::MacroEvent,
//             EventInner::ProgressionEvent(_) => EventType::ProgressionEvent,
//         }
//     }
// }

impl From<&ClientEvent> for Event {
    fn from(client_event: &ClientEvent) -> Event {
        Event {
            event_inner: client_event.event_inner.clone(),
            details: client_event.details.clone(),
            snowflake: client_event.snowflake,
            caused_by: client_event.caused_by.clone(),
        }
    }
}

impl AsRef<Event> for Event {
    fn as_ref(&self) -> &Event {
        self
    }
}

impl Event {
    pub fn is_event_console_message(&self) -> bool {
        self.event_inner.is_console_message()
    }
    pub fn try_player_message(&self) -> Option<(String, String)> {
        match &self.event_inner {
            EventInner::InstanceEvent(instance_event) => match &instance_event.instance_event_inner
            {
                InstanceEventInner::PlayerMessage {
                    player,
                    player_message,
                } => Some((player.clone(), player_message.clone())),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn get_instance_uuid(&self) -> Option<InstanceUuid> {
        event_instance_uuid(&self.event_inner)
    }

    pub fn try_macro_event(&self) -> Option<&MacroEvent> {
        match &self.event_inner {
            EventInner::MacroEvent(macro_event) => Some(macro_event),
            _ => None,
        }
    }

    pub fn new_instance_output(
        instance_uuid: InstanceUuid,
        instance_name: String,
        output: String,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid,
                instance_name,
                instance_event_inner: InstanceEventInner::InstanceOutput { message: output },
            }),
            caused_by: CausedBy::System,
        }
    }

    pub fn new_player_message(
        instance_uuid: InstanceUuid,
        instance_name: String,
        player: String,
        player_message: String,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid,
                instance_name,
                instance_event_inner: InstanceEventInner::PlayerMessage {
                    player,
                    player_message,
                },
            }),
            caused_by: CausedBy::System,
        }
    }

    pub fn new_system_message(
        instance_uuid: InstanceUuid,
        instance_name: String,
        system_message: String,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid,
                instance_name,
                instance_event_inner: InstanceEventInner::SystemMessage {
                    message: system_message,
                },
            }),
            caused_by: CausedBy::System,
        }
    }

    pub fn new_instance_state_transition(
        instance_uuid: InstanceUuid,
        instance_name: String,
        new_state: State,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::InstanceEvent(InstanceEvent {
                instance_uuid,
                instance_name,
                instance_event_inner: InstanceEventInner::StateTransition { to: new_state },
            }),
            caused_by: CausedBy::System,
        }
    }
    #[must_use]
    pub fn new_progression_event_start(
        progression_name: impl AsRef<str>,
        total: Option<f64>,
        inner: Option<ProgressionStartValue>,
        caused_by: CausedBy,
    ) -> (Event, ProgressionEventID) {
        let event_id = ProgressionEventID(Snowflake::default());
        (
            Event {
                details: "".to_string(),
                snowflake: Snowflake::default(),
                event_inner: EventInner::ProgressionEvent(ProgressionEvent {
                    event_id: event_id.0,
                    progression_event_inner: ProgressionEventInner::ProgressionStart {
                        progression_name: progression_name.as_ref().to_string(),
                        total,
                        inner,
                    },
                }),
                caused_by,
            },
            event_id,
        )
    }

    pub fn new_progression_event_update(
        event_id: &ProgressionEventID,
        progress_message: impl AsRef<str>,
        progress: f64,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::ProgressionEvent(ProgressionEvent {
                event_id: event_id.0,
                progression_event_inner: ProgressionEventInner::ProgressionUpdate {
                    progress_message: progress_message.as_ref().to_string(),
                    progress,
                },
            }),
            caused_by: CausedBy::System,
        }
    }

    pub fn new_progression_event_end(
        event_id: ProgressionEventID,
        success: bool,
        message: Option<impl AsRef<str>>,
        inner: Option<ProgressionEndValue>,
    ) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::ProgressionEvent(ProgressionEvent {
                event_id: event_id.0,
                progression_event_inner: ProgressionEventInner::ProgressionEnd {
                    success,
                    message: message.map(|s| s.as_ref().to_string()),
                    inner,
                },
            }),
            caused_by: CausedBy::System,
        }
    }

    pub fn new_macro_detach_event(macro_pid: MacroPID) -> Event {
        Event {
            details: "".to_string(),
            snowflake: Snowflake::default(),
            event_inner: EventInner::MacroEvent(MacroEvent::new(
                None,
                macro_pid,
                "",
                0,
                MacroEventInner::Detach,
            )),
            caused_by: CausedBy::System,
        }
    }
}

#[cfg(test)]
mod query_tests {
    use super::*;
    use crate::auth::permission::UserPermission;
    use crate::auth::user::User;
    use crate::output_types::ClientEvent;
    use crate::traits::t_server::State;

    fn query_for(instance: &str) -> EventQuery {
        EventQuery {
            event_levels: None,
            event_types: None,
            instance_event_types: None,
            user_event_types: None,
            event_user_ids: None,
            event_instance_ids: Some(vec![instance.to_string().into()]),
            bearer_token: None,
            time_range: None,
            exclude_console: true,
        }
    }

    fn state_event(uuid: &str) -> ClientEvent {
        ClientEvent::from(Event::new_instance_state_transition(
            uuid.to_string().into(),
            "test".to_string(),
            State::Running,
        ))
    }

    #[test]
    fn instance_filter_keeps_this_instance_and_drops_another() {
        let query = query_for("INSTANCE_a");
        assert!(query.filter(state_event("INSTANCE_a")));
        assert!(!query.filter(state_event("INSTANCE_b")));
    }

    #[test]
    fn instance_filter_includes_progression_for_that_instance() {
        let query = query_for("INSTANCE_a");
        let end = Event::new_progression_event_end(
            ProgressionEventID(Snowflake::default()),
            true,
            Some("Created"),
            Some(ProgressionEndValue::InstanceDelete {
                instance_uuid: "INSTANCE_a".to_string().into(),
            }),
        );
        let other = Event::new_progression_event_end(
            ProgressionEventID(Snowflake::default()),
            false,
            Some("nope"),
            Some(ProgressionEndValue::InstanceDelete {
                instance_uuid: "INSTANCE_b".to_string().into(),
            }),
        );
        assert!(query.filter(ClientEvent::from(end)));
        assert!(!query.filter(ClientEvent::from(other)));
    }

    #[test]
    fn exclude_console_drops_output_lines() {
        let query = query_for("INSTANCE_a");
        let line = ClientEvent::from(Event::new_instance_output(
            "INSTANCE_a".to_string().into(),
            "test".to_string(),
            "hello".to_string(),
        ));
        assert!(!query.filter(line));
    }

    #[test]
    fn non_owner_sees_progression_only_for_instances_they_can_view() {
        let allowed = InstanceUuid::from("INSTANCE_a".to_string());
        let other = InstanceUuid::from("INSTANCE_b".to_string());
        let mut permissions = UserPermission::default();
        permissions.can_view_instance.insert(allowed.clone());
        let user = User::new("user".to_string(), "12345", false, false, permissions);
        let progression = |uuid: InstanceUuid| {
            Event::new_progression_event_end(
                ProgressionEventID(Snowflake::default()),
                false,
                Some("disk full"),
                Some(ProgressionEndValue::InstanceDelete {
                    instance_uuid: uuid,
                }),
            )
        };
        assert!(user.can_see_event(&progression(allowed)));
        assert!(!user.can_see_event(&progression(other)));
    }

    #[test]
    fn macro_event_keeps_the_task_name_and_old_rows_still_parse() {
        let event = MacroEvent::new(
            None,
            MacroPID(7),
            "backup",
            1_700_000_000,
            MacroEventInner::Started,
        );
        let json = serde_json::to_string(&event).unwrap();
        let parsed: MacroEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_name, "backup");
        assert_eq!(parsed.creation_time, 1_700_000_000);

        let old = r#"{"instance_uuid":null,"macro_pid":7,"macro_event_inner":{"type":"Started"}}"#;
        let legacy: MacroEvent = serde_json::from_str(old).unwrap();
        assert_eq!(legacy.task_name, "");
        assert_eq!(legacy.creation_time, 0);
    }
}
