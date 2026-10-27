use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::battle::{TransitionObservation, TransitionObserver};
use crate::data::DataCatalog;
use crate::model::{ActionRequest, BattleSnapshot, ShieldDecisionRequest, TimelineEvent};
use crate::transition::{TransitionError, resolve_turn_with_shield_decisions};

pub const BATTLE_REPLAY_SCHEMA_VERSION: u32 = 2;
pub const BATTLE_RULES_REVISION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleReplay {
    pub schema_version: u32,
    pub rules_revision: String,
    pub catalog_revision: String,
    pub initial_snapshot: BattleSnapshot,
    pub steps: Vec<ReplayStep>,
}

impl BattleReplay {
    pub fn replay(
        &self,
        catalog: &DataCatalog,
        catalog_revision: &str,
    ) -> Result<BattleSnapshot, ReplayError> {
        if self.schema_version != BATTLE_REPLAY_SCHEMA_VERSION {
            return Err(ReplayError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.rules_revision != BATTLE_RULES_REVISION {
            return Err(ReplayError::RulesRevisionMismatch {
                replay: self.rules_revision.clone(),
                current: BATTLE_RULES_REVISION.to_owned(),
            });
        }
        if self.catalog_revision != catalog_revision {
            return Err(ReplayError::CatalogRevisionMismatch {
                replay: self.catalog_revision.clone(),
                current: catalog_revision.to_owned(),
            });
        }

        let mut snapshot = self.initial_snapshot.clone();
        let mut next_sequence_index = None;
        for (step_index, step) in self.steps.iter().enumerate() {
            if next_sequence_index.is_some_and(|expected| step.sequence_index != expected) {
                return Err(ReplayError::SequenceMismatch {
                    step_index,
                    expected: next_sequence_index.unwrap(),
                    recorded: step.sequence_index,
                });
            }
            next_sequence_index = Some(step.sequence_index.saturating_add(1));
            if snapshot.turn != step.turn_before {
                return Err(ReplayError::TurnMismatch {
                    step_index,
                    expected: step.turn_before,
                    actual: snapshot.turn,
                });
            }

            let transition = resolve_turn_with_shield_decisions(
                &snapshot,
                &step.action_requests,
                &step.shield_decisions,
                catalog,
            )
            .map_err(|source| ReplayError::Transition { step_index, source })?;
            if transition.timeline_entries != step.events {
                return Err(ReplayError::EventMismatch { step_index });
            }
            if transition.next_snapshot.turn != step.turn_after {
                return Err(ReplayError::TurnMismatch {
                    step_index,
                    expected: step.turn_after,
                    actual: transition.next_snapshot.turn,
                });
            }
            snapshot = transition.next_snapshot;
        }

        Ok(snapshot)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayStep {
    pub sequence_index: u64,
    pub turn_before: u32,
    pub turn_after: u32,
    pub action_requests: Vec<ActionRequest>,
    #[serde(default)]
    pub shield_decisions: Vec<ShieldDecisionRequest>,
    pub events: Vec<TimelineEvent>,
}

#[derive(Debug)]
pub enum ReplayRecorderError {
    EmptyCatalogRevision,
}

impl Display for ReplayRecorderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCatalogRevision => formatter.write_str("catalog revision cannot be empty"),
        }
    }
}

impl Error for ReplayRecorderError {}

pub struct ReplayRecorder {
    initial_snapshot: BattleSnapshot,
    catalog_revision: String,
    steps: Vec<ReplayStep>,
}

impl ReplayRecorder {
    pub fn new(
        initial_snapshot: BattleSnapshot,
        catalog_revision: impl Into<String>,
    ) -> Result<Self, ReplayRecorderError> {
        let catalog_revision = catalog_revision.into();
        if catalog_revision.trim().is_empty() {
            return Err(ReplayRecorderError::EmptyCatalogRevision);
        }

        Ok(Self {
            initial_snapshot,
            catalog_revision,
            steps: Vec::new(),
        })
    }

    pub fn finish(self) -> BattleReplay {
        BattleReplay {
            schema_version: BATTLE_REPLAY_SCHEMA_VERSION,
            rules_revision: BATTLE_RULES_REVISION.to_owned(),
            catalog_revision: self.catalog_revision,
            initial_snapshot: self.initial_snapshot,
            steps: self.steps,
        }
    }
}

impl TransitionObserver for ReplayRecorder {
    fn on_transition(&mut self, observation: TransitionObservation<'_>) {
        self.steps.push(ReplayStep {
            sequence_index: observation.sequence_index,
            turn_before: observation.turn_before,
            turn_after: observation.turn_after,
            action_requests: observation.action_requests.to_vec(),
            shield_decisions: observation.shield_decisions.to_vec(),
            events: observation.events.to_vec(),
        });
    }
}

#[derive(Debug)]
pub enum ReplayError {
    UnsupportedSchemaVersion(u32),
    RulesRevisionMismatch {
        replay: String,
        current: String,
    },
    CatalogRevisionMismatch {
        replay: String,
        current: String,
    },
    SequenceMismatch {
        step_index: usize,
        expected: u64,
        recorded: u64,
    },
    TurnMismatch {
        step_index: usize,
        expected: u32,
        actual: u32,
    },
    Transition {
        step_index: usize,
        source: TransitionError,
    },
    EventMismatch {
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
            Self::RulesRevisionMismatch { replay, current } => write!(
                formatter,
                "replay rules revision {replay} does not match current revision {current}"
            ),
            Self::CatalogRevisionMismatch { replay, current } => write!(
                formatter,
                "replay catalog revision {replay} does not match current revision {current}"
            ),
            Self::SequenceMismatch {
                step_index,
                expected,
                recorded,
            } => write!(
                formatter,
                "replay step {step_index} expected sequence index {expected}, got {recorded}"
            ),
            Self::TurnMismatch {
                step_index,
                expected,
                actual,
            } => write!(
                formatter,
                "replay step {step_index} expected turn {expected}, got {actual}"
            ),
            Self::Transition { step_index, source } => {
                write!(formatter, "replay step {step_index} failed: {source}")
            }
            Self::EventMismatch { step_index } => {
                write!(
                    formatter,
                    "replay step {step_index} emitted different events"
                )
            }
        }
    }
}

impl Error for ReplayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transition { source, .. } => Some(source),
            Self::UnsupportedSchemaVersion(_)
            | Self::RulesRevisionMismatch { .. }
            | Self::CatalogRevisionMismatch { .. }
            | Self::SequenceMismatch { .. }
            | Self::TurnMismatch { .. }
            | Self::EventMismatch { .. } => None,
        }
    }
}
