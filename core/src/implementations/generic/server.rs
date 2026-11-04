use tracing::error;

use crate::{
    error::{Error, ErrorKind},
    events::CausedBy,
    traits::t_server::{
        MonitorReport, PlayerAction, PlayerInventory, PlayerLocation, State, TServer,
    },
};

use super::{bridge::procedure_call::ProcedureCallInner, GenericInstance};

#[async_trait::async_trait]
impl TServer for GenericInstance {
    async fn start(&self, caused_by: CausedBy, block: bool) -> Result<(), Error> {
        self.procedure_bridge
            .call(ProcedureCallInner::StartInstance { caused_by, block })
            .await?;
        Ok(())
    }
    async fn stop(&self, caused_by: CausedBy, block: bool) -> Result<(), Error> {
        self.procedure_bridge
            .call(ProcedureCallInner::StopInstance { caused_by, block })
            .await?;
        Ok(())
    }
    async fn restart(&self, caused_by: CausedBy, block: bool) -> Result<(), Error> {
        self.procedure_bridge
            .call(ProcedureCallInner::RestartInstance { caused_by, block })
            .await?;
        Ok(())
    }
    async fn kill(&self, caused_by: CausedBy) -> Result<(), Error> {
        self.procedure_bridge
            .call(ProcedureCallInner::KillInstance { caused_by })
            .await?;
        Ok(())
    }
    async fn state(&self) -> State {
        self.procedure_bridge
            .call(ProcedureCallInner::GetState)
            .await
            .map_err(|e| {
                error!("Failed to get state: {}", e);
                e
            })
            .map_or(State::Stopped, |r| {
                r.try_into()
                    .map_err(|e| {
                        error!("Failed to get state: {}", e);
                        e
                    })
                    .unwrap_or(State::Stopped)
            })
    }
    async fn send_command(&self, command: &str, caused_by: CausedBy) -> Result<(), Error> {
        self.procedure_bridge
            .call(ProcedureCallInner::SendCommand {
                command: command.to_string(),
                caused_by,
            })
            .await?;
        Ok(())
    }
    async fn monitor(&self) -> MonitorReport {
        self.procedure_bridge
            .call(ProcedureCallInner::Monitor)
            .await
            .map_or(MonitorReport::default(), |r| {
                r.try_into().unwrap_or_default()
            })
    }
    async fn player_where(&self, _name: &str) -> Result<PlayerLocation, Error> {
        Ok(PlayerLocation::unavailable(
            "This instance does not support player location.",
        ))
    }
    async fn player_inventory(&self, _name: &str) -> Result<PlayerInventory, Error> {
        Ok(PlayerInventory::unavailable(
            "This instance does not support player inventory.",
        ))
    }
    async fn player_action(&self, _name: &str, _action: PlayerAction) -> Result<String, Error> {
        Err(Error {
            kind: ErrorKind::UnsupportedOperation,
            source: color_eyre::eyre::eyre!("This instance does not support player commands"),
        })
    }
}
