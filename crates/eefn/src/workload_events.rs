//! Bounded metadata from one reviewed microphone source. No grant authority,
//! durable receipt, raw media, source restart or network delivery is implied.
use crate::{
    python::stream::StreamObservation,
    workload_leases::definition::{Definition, EventMode, Events, Overflow, Source},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Sequence counts useful events, while source_sequence retains their position
/// in the original stream. None remains uncertainty, never inferred inactivity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Event {
    pub schema_version: u32,
    pub kind: Kind,
    pub sequence: u64,
    pub source_sequence: u64,
    pub source_frames: u64,
    pub active: Option<bool>,
    pub level: Option<u16>,
    pub discontinuity: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    MicrophoneActivity,
}

pub(crate) struct Filter {
    limits: Events,
    block_frames: u64,
    max_blocks: u64,
    max_source_frames: u64,
    last_source_sequence: u64,
    last_source_frames: u64,
    last_observed_at: Option<Instant>,
    last_active: Option<Option<bool>>,
    last_event_at: Option<Instant>,
    events: u64,
    bytes: usize,
    failed: bool,
}
impl Filter {
    pub(crate) fn new(definition: &Definition) -> Result<Self> {
        definition.validate()?;
        match (&definition.events.mode, &definition.events.overflow) {
            (EventMode::ActivityTransitions, Overflow::Stop) => {}
        }
        let Source::MicrophoneEnergy { parameters, .. } = &definition.source;
        Ok(Self {
            limits: definition.events.clone(),
            block_frames: u64::from(parameters.block_frames),
            max_blocks: u64::from(parameters.max_blocks),
            max_source_frames: definition.limits.max_source_frames,
            last_source_sequence: 0,
            last_source_frames: 0,
            last_observed_at: None,
            last_active: None,
            last_event_at: None,
            events: 0,
            bytes: 0,
            failed: false,
        })
    }

    pub(crate) fn observe(
        &mut self,
        observation: StreamObservation,
        now: Instant,
    ) -> Result<Option<Event>> {
        ensure!(!self.failed, "Workload event reporting already stopped");
        let result = self.accept(observation, now);
        if result.is_err() {
            // Never resume with a later observation after losing a required
            // transition/gap, including if an internal caller catches the error.
            self.failed = true;
        }
        result
    }

    fn accept(&mut self, observation: StreamObservation, now: Instant) -> Result<Option<Event>> {
        ensure!(
            observation.sequence == self.last_source_sequence + 1
                && observation.sequence <= self.max_blocks
                && observation.source_frames == self.last_source_frames + self.block_frames
                && observation.source_frames <= self.max_source_frames
                && observation.level.is_none_or(|v| v <= 32768)
                && (observation.active.is_none() || observation.level.is_some())
                && (!observation.discontinuity
                    || (observation.active.is_none() && observation.level.is_none()))
                && self.last_observed_at.is_none_or(|previous| now >= previous),
            "Workload source observation changed or skipped"
        );
        let required = self.last_active != Some(observation.active) || observation.discontinuity;
        self.last_source_sequence = observation.sequence;
        self.last_source_frames = observation.source_frames;
        self.last_observed_at = Some(now);
        if !required {
            return Ok(None);
        }
        ensure!(
            self.last_event_at.is_none_or(|previous| {
                now.duration_since(previous).as_millis() >= u128::from(self.limits.min_interval_ms)
            }),
            "Workload event rate exceeded; source must stop"
        );
        ensure!(
            self.events < self.limits.max_events,
            "Workload event count exceeded; source must stop"
        );
        let event = Event {
            schema_version: 1,
            kind: Kind::MicrophoneActivity,
            sequence: self.events + 1,
            source_sequence: observation.sequence,
            source_frames: observation.source_frames,
            active: observation.active,
            level: observation.level,
            discontinuity: observation.discontinuity,
        };
        let bytes = serde_json::to_vec(&event)?.len();
        ensure!(
            bytes <= self.limits.max_event_bytes
                && bytes <= self.limits.max_total_bytes.saturating_sub(self.bytes),
            "Workload event bytes exceeded; source must stop"
        );
        self.events += 1;
        self.bytes += bytes;
        self.last_active = Some(event.active);
        self.last_event_at = Some(now);
        Ok(Some(event))
    }
}

#[cfg(test)]
#[path = "workload_events_tests.rs"]
mod tests;
