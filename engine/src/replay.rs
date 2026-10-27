use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::data::DataCatalog;
use crate::model::{ActionRequest, BattleSnapshot, ShieldDecisionRequest};
use crate::transition::{TransitionError, resolve_turn_with_shield_decisions};

pub const BATTLE_REPLAY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleReplay {
    pub schema_version: u32,
    pub initial_snapshot: BattleSnapshot,
    pub steps: Vec<ReplayStep>,
}

impl BattleReplay {
    pub fn new(initial_snapshot: BattleSnapshot, steps: Vec<ReplayStep>) -> Self {
        Self {
            schema_version: BATTLE_REPLAY_SCHEMA_VERSION,
            initial_snapshot,
            steps,
        }
    }

    pub fn replay(&self, catalog: &DataCatalog) -> Result<BattleSnapshot, ReplayError> {
        if self.schema_version != BATTLE_REPLAY_SCHEMA_VERSION {
            return Err(ReplayError::UnsupportedSchemaVersion(self.schema_version));
        }

        let mut snapshot = self.initial_snapshot.clone();
        for (step_index, step) in self.steps.iter().enumerate() {
            let transition = resolve_turn_with_shield_decisions(
                &snapshot,
                &step.action_requests,
                &step.shield_decisions,
                catalog,
            )
            .map_err(|source| ReplayError::Transition { step_index, source })?;
            if transition.next_snapshot != step.resulting_snapshot {
                return Err(ReplayError::SnapshotMismatch { step_index });
            }
            snapshot = transition.next_snapshot;
        }

        Ok(snapshot)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayStep {
    pub action_requests: Vec<ActionRequest>,
    #[serde(default)]
    pub shield_decisions: Vec<ShieldDecisionRequest>,
    pub resulting_snapshot: BattleSnapshot,
}

#[derive(Debug)]
pub enum ReplayError {
    UnsupportedSchemaVersion(u32),
    Transition {
        step_index: usize,
        source: TransitionError,
    },
    SnapshotMismatch {
        step_index: usize,
    },
}

impl Display for ReplayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported battle replay schema version: {version}"
                )
            }
            Self::Transition { step_index, source } => {
                write!(formatter, "replay step {step_index} failed: {source}")
            }
            Self::SnapshotMismatch { step_index } => {
                write!(
                    formatter,
                    "replay step {step_index} did not match its recorded snapshot"
                )
            }
        }
    }
}

impl Error for ReplayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transition { source, .. } => Some(source),
            Self::UnsupportedSchemaVersion(_) | Self::SnapshotMismatch { .. } => None,
        }
    }
}
