//! Opt-in exact presentation-time sidecar for the existing seconds-only v1 composition.
//!
//! Foundation owns all rational-time validation and arithmetic. The sidecar
//! preserves producer PTS and timebases without modifying v1 evidence JSON or
//! claiming that seconds-only producers can reconstruct their original PTS.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use media_core::{MediaRange, Timestamp};
use serde::{Deserialize, Serialize};

use crate::{
    compose, CompositionInput, EvidenceAnalysis, MediaIdentity, Modality, ModalityInput,
    ProducerReceipt,
};

pub const EXACT_TIMING_SCHEMA: &str = "media-intelligence.exact-timing";
pub const EXACT_TIMING_SCHEMA_VERSION: u32 = 1;

/// A sidecar is meaningful only for exact, already aligned, media-relative PTS.
/// Other source clocks require an explicit exact alignment before using this API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactTimingSidecarV1 {
    pub schema: String,
    pub schema_version: u32,
    pub media: MediaIdentity,
    /// The source's v1 batch revision, when this sidecar accompanies a batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
    /// Must be `media_relative`; raw stream clocks cannot be compared yet.
    pub clock: String,
    /// Modalities whose *every* source record has an original exact anchor.
    /// Other modalities remain explicitly seconds-only in the legacy payload.
    pub exact_modalities: Vec<Modality>,
    pub anchors: Vec<ExactAnchorV1>,
}

/// One source record, identified by its full producer receipt and local id.
/// Speech uses the decimal segment index as its `record_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAnchorV1 {
    pub receipt: ProducerReceipt,
    pub record_id: String,
    pub timing: ExactAnchorTimingV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_frame: Option<u64>,
}

/// Foundation `Timestamp` wire values; `MediaRange` validates half-open ranges.
/// This is a serialization adapter, not a second timestamp/arithmetic model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExactAnchorTimingV1 {
    Instant { timestamp: Timestamp },
    Range { start: Timestamp, end: Timestamp },
}

impl ExactAnchorTimingV1 {
    fn checked_seconds(self) -> Result<(f64, f64), ExactTimingError> {
        let (start, end) = match self {
            Self::Instant { timestamp } => {
                timestamp.validate().map_err(invalid_time)?;
                (timestamp, timestamp)
            }
            Self::Range { start, end } => {
                MediaRange::new(start, end).map_err(invalid_time)?;
                (start, end)
            }
        };
        let seconds = (start.seconds(), end.seconds());
        if !seconds.0.is_finite() || !seconds.1.is_finite() || seconds.0 < 0.0 {
            return Err(ExactTimingError(
                "exact anchors must be finite and media-relative".into(),
            ));
        }
        Ok(seconds)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactTimingError(pub String);

impl Display for ExactTimingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ExactTimingError {}

fn invalid_time(error: impl Display) -> ExactTimingError {
    ExactTimingError(format!("invalid Foundation timestamp: {error}"))
}

type AnchorKey = (Modality, String, String);

fn anchor_key(anchor: &ExactAnchorV1) -> Result<AnchorKey, ExactTimingError> {
    let receipt = serde_json::to_string(&anchor.receipt).map_err(invalid_time)?;
    Ok((anchor.receipt.modality, receipt, anchor.record_id.clone()))
}

#[derive(Debug, Clone, Copy)]
struct ExpectedAnchor {
    seconds: (f64, f64),
    frame_index: Option<u64>,
    start_frame: Option<u64>,
    end_frame: Option<u64>,
    instant: bool,
}

fn expect_record(
    records: &mut BTreeMap<AnchorKey, ExpectedAnchor>,
    receipt: &ProducerReceipt,
    id: String,
    expected: ExpectedAnchor,
) -> Result<(), ExactTimingError> {
    let key = (
        receipt.modality,
        serde_json::to_string(receipt).map_err(invalid_time)?,
        id,
    );
    if records.insert(key, expected).is_some() {
        return Err(ExactTimingError(
            "duplicate producer-local evidence id".into(),
        ));
    }
    Ok(())
}

impl ExactTimingSidecarV1 {
    /// Construct in deterministic order. This does not invent any timestamps.
    pub fn new(
        media: MediaIdentity,
        base_revision: Option<String>,
        exact_modalities: Vec<Modality>,
        anchors: Vec<ExactAnchorV1>,
    ) -> Result<Self, ExactTimingError> {
        let mut exact_modalities = exact_modalities;
        exact_modalities.sort();
        exact_modalities.dedup();
        let mut keyed = anchors
            .into_iter()
            .map(|anchor| Ok((anchor_key(&anchor)?, anchor)))
            .collect::<Result<Vec<_>, ExactTimingError>>()?;
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            schema: EXACT_TIMING_SCHEMA.into(),
            schema_version: EXACT_TIMING_SCHEMA_VERSION,
            media,
            base_revision,
            clock: "media_relative".into(),
            exact_modalities,
            anchors: keyed.into_iter().map(|(_, anchor)| anchor).collect(),
        })
    }

    /// Verify that this was captured alongside the named v1 batch revision.
    pub fn validate_batch_revision(&self, revision: &str) -> Result<(), ExactTimingError> {
        if self.base_revision.as_deref() != Some(revision) || revision.trim().is_empty() {
            return Err(ExactTimingError(
                "exact timing sidecar has a different v1 batch revision".into(),
            ));
        }
        Ok(())
    }

    /// Require one original exact anchor per record of every declared modality,
    /// with no extras. Undeclared modalities retain their honest v1-only timing.
    /// In particular, no adapter may reconstruct a missing PTS from v1 seconds.
    pub fn validate_for(&self, input: &CompositionInput) -> Result<(), ExactTimingError> {
        if self.schema != EXACT_TIMING_SCHEMA
            || self.schema_version != EXACT_TIMING_SCHEMA_VERSION
            || self.clock != "media_relative"
            || self.media != input.media
            || self.exact_modalities.is_empty()
            || self
                .exact_modalities
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .base_revision
                .as_deref()
                .is_some_and(|revision| revision.trim().is_empty())
        {
            return Err(ExactTimingError(
                "invalid exact timing header or source identity".into(),
            ));
        }
        let mut records = BTreeMap::new();
        let exact_scenes = match &input.scenes {
            ModalityInput::Completed { runs }
                if self.exact_modalities.contains(&Modality::Scene) =>
            {
                runs.as_slice()
            }
            _ => &[],
        };
        for run in exact_scenes {
            for scene in &run.scenes {
                expect_record(
                    &mut records,
                    &run.receipt,
                    scene.id.clone(),
                    ExpectedAnchor {
                        seconds: (scene.start_seconds, scene.end_seconds),
                        frame_index: None,
                        start_frame: scene.start_frame,
                        end_frame: scene.end_frame,
                        instant: false,
                    },
                )?;
            }
        }
        let exact_ocr = match &input.visible_text {
            ModalityInput::Completed { runs }
                if self.exact_modalities.contains(&Modality::VisibleText) =>
            {
                runs.as_slice()
            }
            _ => &[],
        };
        for run in exact_ocr {
            for observation in &run.observations {
                expect_record(
                    &mut records,
                    &run.receipt,
                    observation.id.clone(),
                    ExpectedAnchor {
                        seconds: (observation.timestamp_seconds, observation.timestamp_seconds),
                        frame_index: observation.frame_index,
                        start_frame: None,
                        end_frame: None,
                        instant: true,
                    },
                )?;
            }
        }
        let exact_speech = match &input.speech {
            ModalityInput::Completed { runs }
                if self.exact_modalities.contains(&Modality::Speech) =>
            {
                runs.as_slice()
            }
            _ => &[],
        };
        for run in exact_speech {
            for segment in &run.segments {
                expect_record(
                    &mut records,
                    &run.receipt,
                    segment.index.to_string(),
                    ExpectedAnchor {
                        seconds: (segment.start_seconds, segment.end_seconds),
                        frame_index: None,
                        start_frame: None,
                        end_frame: None,
                        instant: false,
                    },
                )?;
            }
        }
        let mut previous: Option<AnchorKey> = None;
        let mut anchored_modalities = Vec::new();
        for anchor in &self.anchors {
            if anchor.record_id.trim().is_empty() {
                return Err(ExactTimingError("empty exact timing record id".into()));
            }
            let key = anchor_key(anchor)?;
            if previous.as_ref().is_some_and(|prev| prev >= &key) {
                return Err(ExactTimingError(
                    "exact timing anchors are unsorted or duplicated".into(),
                ));
            }
            previous = Some(key.clone());
            if anchored_modalities.last() != Some(&anchor.receipt.modality) {
                anchored_modalities.push(anchor.receipt.modality);
            }
            let expected = records.remove(&key).ok_or_else(|| {
                ExactTimingError(format!("unknown exact timing record {}", anchor.record_id))
            })?;
            let instant = matches!(anchor.timing, ExactAnchorTimingV1::Instant { .. });
            if instant != expected.instant
                || anchor.frame_index != expected.frame_index
                || anchor.start_frame != expected.start_frame
                || anchor.end_frame != expected.end_frame
                || anchor.timing.checked_seconds()? != expected.seconds
            {
                return Err(ExactTimingError(format!(
                    "exact timing disagrees with v1 projection for {}",
                    anchor.record_id
                )));
            }
        }
        if !records.is_empty() {
            return Err(ExactTimingError(
                "some declared-exact source records lack original PTS anchors".into(),
            ));
        }
        anchored_modalities.sort();
        anchored_modalities.dedup();
        if anchored_modalities != self.exact_modalities {
            return Err(ExactTimingError(
                "some declared-exact modalities have no source anchors".into(),
            ));
        }
        Ok(())
    }
}

/// A distinct opt-in envelope; its `legacy` member is ordinary unchanged v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvidenceAnalysisV1 {
    pub legacy: EvidenceAnalysis,
    pub exact_timing: ExactTimingSidecarV1,
}

/// Compose without altering v1 payloads. Lossless output requires actual PTS
/// from producers; the normal `compose` entry point remains seconds-only.
pub fn compose_with_exact_timing(
    input: &CompositionInput,
    exact_timing: ExactTimingSidecarV1,
) -> Result<ExactEvidenceAnalysisV1, ExactTimingError> {
    exact_timing.validate_for(input)?;
    let legacy = compose(input).map_err(invalid_time)?;
    Ok(ExactEvidenceAnalysisV1 {
        legacy,
        exact_timing,
    })
}
