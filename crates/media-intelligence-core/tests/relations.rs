//! Scene→speech/visible-text relations and speech↔visible-text co-occurrence by
//! half-open time overlap.

mod common;

use std::collections::BTreeSet;

use common::{labelled_relations, lecture};
use media_intelligence_core::{compose, RelationKind};

fn rel(kind: RelationKind, subject: &str, object: &str) -> (RelationKind, String, String) {
    (kind, subject.to_string(), object.to_string())
}

#[test]
fn relations_are_exactly_the_half_open_overlaps() {
    use RelationKind::*;
    let analysis = compose(&lecture()).unwrap();

    let expected: BTreeSet<_> = [
        // Scene → speech. speech:2 [10,12) touches sc-0 [0,10) and is not related to it.
        rel(SceneSpeech, "scene:sc-0", "speech:0"),
        rel(SceneSpeech, "scene:sc-0", "speech:1"),
        rel(SceneSpeech, "scene:sc-1", "speech:2"),
        rel(SceneSpeech, "scene:sc-1", "speech:3"),
        rel(SceneSpeech, "scene:sc-1", "speech:4"),
        rel(SceneSpeech, "scene:sc-1", "speech:5"),
        rel(SceneSpeech, "scene:sc-2", "speech:5"),
        // Scene → visible text. A track crossing a cut relates to both scenes.
        rel(SceneVisibleText, "scene:sc-0", "track:trk-title"),
        rel(SceneVisibleText, "scene:sc-1", "obs:ocr-main-1"),
        rel(SceneVisibleText, "scene:sc-1", "track:trk-slide"),
        rel(SceneVisibleText, "scene:sc-2", "track:trk-slide"),
        rel(SceneVisibleText, "scene:sc-2", "obs:ocr-main-2"),
        // Speech ↔ visible text.
        rel(SpeechVisibleTextCooccurrence, "speech:0", "track:trk-title"),
        rel(SpeechVisibleTextCooccurrence, "speech:1", "track:trk-title"),
        rel(SpeechVisibleTextCooccurrence, "speech:3", "obs:ocr-main-1"),
        rel(SpeechVisibleTextCooccurrence, "speech:5", "track:trk-slide"),
        rel(SpeechVisibleTextCooccurrence, "speech:5", "obs:ocr-main-2"),
    ]
    .into_iter()
    .collect();

    assert_eq!(labelled_relations(&analysis), expected);
}

#[test]
fn touching_intervals_are_not_related() {
    use RelationKind::*;
    let relations = labelled_relations(&compose(&lecture()).unwrap());

    // speech:2 = [10,12) ends where the instant ocr-main-1 (t=12) is sampled.
    assert!(!relations.contains(&rel(
        SpeechVisibleTextCooccurrence,
        "speech:2",
        "obs:ocr-main-1"
    )));
    // speech:4 = [16,18) ends where trk-slide [18,22] starts.
    assert!(!relations.contains(&rel(
        SpeechVisibleTextCooccurrence,
        "speech:4",
        "track:trk-slide"
    )));
    // speech:2 starts where sc-0 [0,10) ends.
    assert!(!relations.contains(&rel(SceneSpeech, "scene:sc-0", "speech:2")));
    // An instant at an interval's start is inside it: ocr-main-1 (t=12) and speech:3 [12,16).
    assert!(relations.contains(&rel(
        SpeechVisibleTextCooccurrence,
        "speech:3",
        "obs:ocr-main-1"
    )));
}

#[test]
fn relations_are_sorted_unique_and_reference_existing_evidence() {
    let analysis = compose(&lecture()).unwrap();
    let mut sorted = analysis.relations.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        analysis.relations, sorted,
        "relations are sorted and duplicate-free"
    );

    let ids: BTreeSet<_> = analysis.evidence.iter().map(|e| e.id.as_str()).collect();
    for relation in &analysis.relations {
        assert!(ids.contains(relation.subject.as_str()));
        assert!(ids.contains(relation.object.as_str()));
    }
}
