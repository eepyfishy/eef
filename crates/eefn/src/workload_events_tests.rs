use super::*;
use serde_json::json;
use std::time::Duration;

#[test]
fn final_filter_seal_counts_quiet_tail_and_cannot_be_reused() {
    let mut filter = Filter::new(&definition()).unwrap();
    let start = Instant::now();
    let first = filter
        .observe(observation(1, Some(false)), start)
        .unwrap()
        .unwrap();
    assert!(
        filter
            .observe(observation(2, Some(false)), at(start, 1))
            .unwrap()
            .is_none()
    );
    let summary = crate::python::stream::StreamSummary {
        stream_id: uuid::Uuid::new_v4().to_string(),
        observations: 2,
        source_frames: 160,
    };
    let seal = filter.seal(&summary).unwrap();
    assert!(seal.confirms(1, serde_json::to_vec(&first).unwrap().len(), &first));
    assert!(!seal.confirms(2, serde_json::to_vec(&first).unwrap().len(), &first));
    assert!(filter.seal(&summary).is_err());
    assert!(
        filter
            .observe(observation(3, Some(false)), at(start, 2))
            .is_err()
    );
}
#[test]
fn missing_observations_or_failed_filter_cannot_be_sealed_as_completed() {
    let mut filter = Filter::new(&definition()).unwrap();
    filter
        .observe(observation(1, Some(false)), Instant::now())
        .unwrap();
    let mut summary = crate::python::stream::StreamSummary {
        stream_id: uuid::Uuid::new_v4().to_string(),
        observations: 2,
        source_frames: 160,
    };
    assert!(filter.seal(&summary).is_err());
    summary.observations = 1;
    summary.source_frames = 80;
    assert!(
        filter.seal(&summary).is_err(),
        "An invalid finish cannot retry a retired filter"
    );
    let mut filter = Filter::new(&definition()).unwrap();
    assert!(
        filter
            .observe(observation(2, Some(false)), Instant::now())
            .is_err()
    );
    assert!(filter.seal(&summary).is_err());
}

fn definition() -> Definition {
    serde_json::from_value(json!({
        "schema_version":2,"grant_id":uuid::Uuid::new_v4().to_string(),
        "issuer_id":"a".repeat(64),"session_id":uuid::Uuid::new_v4().to_string(),
        "executor_node":"node","job_id":"b".repeat(32),"task_id":"c".repeat(32),
        "attempt":1,"plan_sha256":"d".repeat(64),
        "request_context":{"request_id":uuid::Uuid::new_v4().to_string(),"origin_node":"origin","origin_area":["home"]},
        "priority":0,"source":{"kind":"microphone_energy","resource_id":"node::mic",
            "resource_sha256":"e".repeat(64),"adapter_sha256":"f".repeat(64),
            "parameters":{"sample_rate":16000,"channels":1,"block_frames":80,"max_blocks":100,
                "activation_level":1000,"deactivation_level":500}},
        "limits":{"max_session_ms":60000,"max_lease_ms":10000,"max_job_memory_mib":128,"max_source_frames":8000},
        "events":{"mode":"activity_transitions","min_interval_ms":100,"max_events":100,
            "max_event_bytes":1024,"max_total_bytes":65536,"max_pending":4,"overflow":"stop"}
    })).unwrap()
}
fn observation(sequence: u64, active: Option<bool>) -> StreamObservation {
    StreamObservation {
        sequence,
        source_frames: sequence * 80,
        active,
        level: Some(if active == Some(true) { 1200 } else { 200 }),
        discontinuity: false,
    }
}
fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

#[test]
fn repeated_levels_are_suppressed_without_renumbering_source_positions() {
    let mut filter = Filter::new(&definition()).unwrap();
    let start = Instant::now();
    let first = filter
        .observe(observation(1, Some(false)), start)
        .unwrap()
        .unwrap();
    let mut repeated = observation(2, Some(false));
    repeated.level = Some(400);
    assert!(filter.observe(repeated, at(start, 1)).unwrap().is_none());
    let changed = filter
        .observe(observation(3, Some(true)), at(start, 100))
        .unwrap()
        .unwrap();
    assert_eq!((first.sequence, first.source_sequence), (1, 1));
    assert_eq!(
        (
            changed.sequence,
            changed.source_sequence,
            changed.source_frames
        ),
        (2, 3, 240)
    );
    assert_eq!(filter.events, 2);
    assert_eq!(
        filter.bytes,
        serde_json::to_vec(&first).unwrap().len() + serde_json::to_vec(&changed).unwrap().len()
    );
    let value = json!(changed);
    assert_eq!(value.as_object().unwrap().len(), 9);
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["elapsed_ms"], 100);
    assert_eq!(value["kind"], "microphone_activity");
    assert!(value.get("data_base64").is_none());
    assert!(value.get("device").is_none());
}

#[test]
fn unknown_and_each_gap_survive_until_a_new_observed_state() {
    let mut filter = Filter::new(&definition()).unwrap();
    let start = Instant::now();
    let initial = filter
        .observe(observation(1, None), start)
        .unwrap()
        .unwrap();
    assert_eq!(initial.active, None);
    assert!(!initial.discontinuity);
    assert!(
        filter
            .observe(observation(2, None), at(start, 50))
            .unwrap()
            .is_none()
    );
    filter
        .observe(observation(3, Some(true)), at(start, 100))
        .unwrap()
        .unwrap();
    for n in 4..=5 {
        let mut gap = observation(n, None);
        gap.level = None;
        gap.discontinuity = true;
        let event = filter
            .observe(gap, at(start, (n - 2) * 100))
            .unwrap()
            .unwrap();
        assert_eq!(event.active, None);
        assert_eq!(event.level, None);
        assert!(event.discontinuity);
    }
    let quiet = filter
        .observe(observation(6, Some(false)), at(start, 400))
        .unwrap()
        .unwrap();
    assert_eq!(quiet.sequence, 5);
    assert_eq!(quiet.active, Some(false));
}

#[test]
fn rate_boundary_stops_instead_of_silently_losing_a_transition() {
    let start = Instant::now();
    let mut filter = Filter::new(&definition()).unwrap();
    filter.observe(observation(1, Some(false)), start).unwrap();
    assert!(
        filter
            .observe(observation(2, Some(true)), at(start, 99))
            .unwrap_err()
            .to_string()
            .contains("rate")
    );
    assert!(
        filter
            .observe(observation(3, Some(true)), at(start, 200))
            .is_err()
    );
    assert_eq!(filter.events, 1);
    let mut exact = Filter::new(&definition()).unwrap();
    exact.observe(observation(1, Some(false)), start).unwrap();
    assert!(
        exact
            .observe(observation(2, Some(true)), at(start, 100))
            .unwrap()
            .is_some()
    );
}

#[test]
fn suppressed_observations_do_not_consume_event_count_but_overflow_is_permanent() {
    let mut d = definition();
    d.events.max_events = 2;
    let mut filter = Filter::new(&d).unwrap();
    let start = Instant::now();
    filter.observe(observation(1, Some(false)), start).unwrap();
    assert!(
        filter
            .observe(observation(2, Some(false)), at(start, 1))
            .unwrap()
            .is_none()
    );
    filter
        .observe(observation(3, Some(true)), at(start, 100))
        .unwrap();
    assert!(
        filter
            .observe(observation(4, Some(false)), at(start, 200))
            .unwrap_err()
            .to_string()
            .contains("count")
    );
    assert_eq!(filter.events, 2);
    assert!(
        filter
            .observe(observation(5, Some(true)), at(start, 300))
            .is_err()
    );
}

#[test]
fn per_event_budget_counts_the_exact_encoded_metadata_before_delivery() {
    let start = Instant::now();
    let d = definition();
    let event = Filter::new(&d)
        .unwrap()
        .observe(observation(1, Some(true)), start)
        .unwrap()
        .unwrap();
    let bytes = serde_json::to_vec(&event).unwrap().len();
    assert!(bytes > 128);
    let mut exact = d.clone();
    exact.events.max_event_bytes = bytes;
    assert_eq!(
        Filter::new(&exact)
            .unwrap()
            .observe(observation(1, Some(true)), start)
            .unwrap(),
        Some(event)
    );
    exact.events.max_event_bytes = bytes - 1;
    let mut too_small = Filter::new(&exact).unwrap();
    assert!(
        too_small
            .observe(observation(1, Some(true)), start)
            .unwrap_err()
            .to_string()
            .contains("bytes")
    );
    assert_eq!((too_small.events, too_small.bytes), (0, 0));
}

#[test]
fn aggregate_budget_accepts_exact_bytes_and_refuses_one_less_without_partial_event() {
    let start = Instant::now();
    let mut trial = Filter::new(&definition()).unwrap();
    for n in 1..=8 {
        trial
            .observe(observation(n, Some(n % 2 == 0)), at(start, n * 100))
            .unwrap();
    }
    let bytes = trial.bytes;
    assert!(bytes > 1024);
    for (budget, expected) in [(bytes, 8), (bytes - 1, 7)] {
        let mut d = definition();
        d.events.max_total_bytes = budget;
        let mut filter = Filter::new(&d).unwrap();
        for n in 1..=8 {
            let result = filter.observe(observation(n, Some(n % 2 == 0)), at(start, n * 100));
            assert_eq!(result.is_ok(), n <= expected);
        }
        assert_eq!(filter.events, expected);
        assert!(filter.bytes <= budget);
        assert!(
            filter
                .observe(observation(9, Some(false)), at(start, 900))
                .is_err()
        );
    }
}

#[test]
fn invalid_or_skipped_source_observations_stop_before_any_new_event() {
    let start = Instant::now();
    let normal = observation(2, Some(true));
    let mut invalid = vec![observation(1, Some(true)), observation(3, Some(true))];
    let mut frames = normal.clone();
    frames.source_frames += 1;
    invalid.push(frames);
    let mut level = normal.clone();
    level.level = Some(32769);
    invalid.push(level);
    let mut absent = normal.clone();
    absent.level = None;
    invalid.push(absent);
    let mut gap = normal.clone();
    gap.discontinuity = true;
    invalid.push(gap);
    let mut false_gap = observation(2, None);
    false_gap.discontinuity = true;
    invalid.push(false_gap);
    for value in invalid {
        let mut filter = Filter::new(&definition()).unwrap();
        filter.observe(observation(1, Some(false)), start).unwrap();
        assert!(filter.observe(value, at(start, 100)).is_err());
        assert_eq!(filter.events, 1);
        assert!(filter.observe(normal.clone(), at(start, 200)).is_err());
    }
}

#[test]
fn repeated_state_cannot_hide_source_limit_or_monotonic_clock_failure() {
    let start = Instant::now();
    let mut d = definition();
    let Source::MicrophoneEnergy { parameters, .. } = &mut d.source;
    parameters.max_blocks = 1;
    d.limits.max_source_frames = 80;
    let mut filter = Filter::new(&d).unwrap();
    filter.observe(observation(1, Some(false)), start).unwrap();
    assert!(
        filter
            .observe(observation(2, Some(false)), at(start, 100))
            .is_err()
    );
    let mut filter = Filter::new(&definition()).unwrap();
    filter
        .observe(observation(1, Some(false)), at(start, 100))
        .unwrap();
    assert!(filter.observe(observation(2, Some(false)), start).is_err());
}
