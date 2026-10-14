//! Shared helpers for the #25 acceptance tests.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use media_intelligence_core::{
    CompositionInput, CoverageStatus, EvidenceAnalysis, EvidenceContent, EvidenceItem, Modality,
    ModalityCoverage, ModalityInput, RelationKind,
};

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).expect("fixture is readable")
}

pub fn load_input(name: &str) -> CompositionInput {
    serde_json::from_str(&fixture_text(name)).expect("fixture deserializes as CompositionInput")
}

/// Mixed speech + scenes + tracked/untracked OCR for one video.
pub fn lecture() -> CompositionInput {
    load_input("lecture_composition.json")
}

/// Producer-facing label for an item, independent of the core's evidence-id scheme.
pub fn label(item: &EvidenceItem) -> String {
    match &item.content {
        EvidenceContent::Speech(speech) => format!("speech:{}", speech.segment_index),
        EvidenceContent::Scene(scene) => format!("scene:{}", scene.scene_id),
        EvidenceContent::VisibleText(text) => match &text.track_id {
            Some(track) => format!("track:{track}"),
            None => format!(
                "obs:{}",
                text.observations
                    .first()
                    .expect("visible text has at least one observation")
                    .observation_id
            ),
        },
    }
}

/// Label qualified by the producing model version (for multi-revision inputs).
pub fn revision_label(item: &EvidenceItem) -> String {
    format!("{}@{}", label(item), item.receipt.model_version)
}

pub fn labels(analysis: &EvidenceAnalysis) -> Vec<String> {
    analysis.evidence.iter().map(label).collect()
}

pub fn item<'a>(analysis: &'a EvidenceAnalysis, wanted: &str) -> &'a EvidenceItem {
    let matches: Vec<_> = analysis
        .evidence
        .iter()
        .filter(|item| label(item) == wanted)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one item labelled {wanted}"
    );
    matches[0]
}

pub fn id_to_label(analysis: &EvidenceAnalysis) -> BTreeMap<String, String> {
    analysis
        .evidence
        .iter()
        .map(|item| (item.id.clone(), label(item)))
        .collect()
}

/// Relations rewritten from evidence ids to producer labels.
pub fn labelled_relations(analysis: &EvidenceAnalysis) -> BTreeSet<(RelationKind, String, String)> {
    let labels = id_to_label(analysis);
    analysis
        .relations
        .iter()
        .map(|relation| {
            (
                relation.kind,
                labels
                    .get(&relation.subject)
                    .unwrap_or_else(|| {
                        panic!("relation subject {} is an evidence id", relation.subject)
                    })
                    .clone(),
                labels
                    .get(&relation.object)
                    .unwrap_or_else(|| {
                        panic!("relation object {} is an evidence id", relation.object)
                    })
                    .clone(),
            )
        })
        .collect()
}

pub fn ids_of(analysis: &EvidenceAnalysis, modality: Modality) -> BTreeSet<String> {
    analysis
        .evidence
        .iter()
        .filter(|item| item.modality == modality)
        .map(|item| item.id.clone())
        .collect()
}

pub fn coverage(analysis: &EvidenceAnalysis, modality: Modality) -> &CoverageStatus {
    let entries: Vec<&ModalityCoverage> = analysis
        .coverage
        .iter()
        .filter(|entry| entry.modality == modality)
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "exactly one coverage entry for {modality:?}"
    );
    &entries[0].status
}

pub fn serialized(analysis: &EvidenceAnalysis) -> Vec<u8> {
    serde_json::to_vec(analysis).expect("analysis serializes")
}

/// Deterministic permutation without a RNG dependency: reverse, then rotate by one.
pub fn permute<T>(values: &mut [T]) {
    values.reverse();
    if values.len() > 1 {
        values.rotate_left(1);
    }
}

/// Permutes every input collection, including run lists and track membership lists.
pub fn shuffle_input(input: &mut CompositionInput) {
    if let ModalityInput::Completed { runs } = &mut input.speech {
        permute(runs);
        for run in runs.iter_mut() {
            permute(&mut run.segments);
        }
    }
    if let ModalityInput::Completed { runs } = &mut input.scenes {
        permute(runs);
        for run in runs.iter_mut() {
            permute(&mut run.scenes);
        }
    }
    if let ModalityInput::Completed { runs } = &mut input.visible_text {
        permute(runs);
        for run in runs.iter_mut() {
            permute(&mut run.observations);
            permute(&mut run.tracks);
            for track in run.tracks.iter_mut() {
                permute(&mut track.observation_ids);
            }
        }
    }
}
