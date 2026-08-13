use super::{
    Completeness, HistoryQuery, QueryCandidate, QueryCoverage, QueryGap, QueryRange, QuerySelector,
    StreamWatermark,
};

#[test]
fn healthiest_complete_repository_wins_with_coverage_watermark_and_skew() {
    let request = HistoryQuery::new(100, 200, 100).expect("bounded query");
    let complete = candidate("repo-a", 0, vec![]);
    let partial = candidate(
        "repo-b",
        0,
        vec![QueryGap::new(130, 140, false).expect("gap")],
    );
    let plan = QuerySelector::select(&request, [partial, complete]).expect("query plan");

    assert_eq!(plan.completeness(), Completeness::Complete);
    assert_eq!(plan.repository_id(), Some("repo-a"));
    assert_eq!(
        plan.coverage().expect("coverage").observed(),
        QueryRange::new(100, 200).expect("range")
    );
    assert_eq!(plan.watermarks()[0].sequence(), 200);
    assert_eq!(plan.clock_skew_seconds(), 0);
}

#[test]
fn partial_and_local_only_results_expose_explicit_gaps_without_unbounded_exports() {
    assert!(HistoryQuery::new(0, 10, 1_001).is_err());
    let request = HistoryQuery::new(100, 200, 32).expect("bounded query");
    let gap = QueryGap::new(120, 130, true).expect("permanent gap");
    let partial =
        QuerySelector::select(&request, [candidate("repo-a", 3, vec![gap])]).expect("query plan");
    assert_eq!(partial.completeness(), Completeness::Partial);
    assert!(partial.gaps()[0].permanent());
    assert_eq!(partial.page_size(), 32);

    let local = QuerySelector::select(
        &request,
        [
            QueryCandidate::unavailable("repo-a"),
            QueryCandidate::unready("repo-b"),
        ],
    )
    .expect("local plan");
    assert_eq!(local.completeness(), Completeness::LocalOnly);
    assert_eq!(local.repository_id(), None);
}

#[test]
fn selection_prefers_requested_coverage_over_a_gap_free_but_short_repository() {
    let request = HistoryQuery::new(100, 200, 32).expect("bounded query");
    let short = QueryCandidate::ready(
        "repo-short",
        QueryCoverage::new(
            QueryRange::new(100, 150).expect("observed"),
            QueryRange::new(100, 150).expect("received"),
        ),
        vec![StreamWatermark::new("source-a", 1, "traffic", 150).expect("watermark")],
        vec![],
        0,
    )
    .expect("candidate");
    let covered = candidate("repo-covered", 0, vec![]);

    let plan = QuerySelector::select(&request, [short, covered]).expect("query plan");
    assert_eq!(plan.repository_id(), Some("repo-covered"));
    assert_eq!(plan.completeness(), Completeness::Complete);
}

#[test]
fn received_coverage_must_match_observed_coverage_for_a_complete_result() {
    let request = HistoryQuery::new(100, 200, 32).expect("bounded query");
    let lagging = QueryCandidate::ready(
        "repo-lagging",
        QueryCoverage::new(
            QueryRange::new(100, 200).expect("observed"),
            QueryRange::new(100, 150).expect("received"),
        ),
        vec![StreamWatermark::new("source-a", 1, "traffic", 150).expect("watermark")],
        vec![],
        0,
    )
    .expect("candidate");

    let plan = QuerySelector::select(&request, [lagging]).expect("query plan");
    assert_eq!(plan.completeness(), Completeness::Partial);
}

#[test]
fn partial_selection_prefers_the_largest_effective_coverage_and_rejects_unbounded_metadata() {
    let request = HistoryQuery::new(100, 200, 32).expect("bounded query");
    let short = QueryCandidate::ready(
        "repo-short",
        QueryCoverage::new(
            QueryRange::new(100, 150).expect("observed"),
            QueryRange::new(100, 150).expect("received"),
        ),
        [],
        [],
        0,
    )
    .expect("candidate");
    let longer = QueryCandidate::ready(
        "repo-longer",
        QueryCoverage::new(
            QueryRange::new(100, 180).expect("observed"),
            QueryRange::new(100, 180).expect("received"),
        ),
        [],
        [],
        1,
    )
    .expect("candidate");

    let plan = QuerySelector::select(&request, [short, longer]).expect("query plan");
    assert_eq!(plan.repository_id(), Some("repo-longer"));
    assert_eq!(plan.completeness(), Completeness::Partial);

    let watermarks = (0..257).map(|sequence| {
        StreamWatermark::new("source-a", 1, "traffic", sequence).expect("watermark")
    });
    assert!(
        QueryCandidate::ready(
            "repo-overflow",
            QueryCoverage::new(
                QueryRange::new(100, 200).expect("observed"),
                QueryRange::new(100, 200).expect("received"),
            ),
            watermarks,
            [],
            0,
        )
        .is_err()
    );
}

fn candidate(repository_id: &str, skew: i64, gaps: Vec<QueryGap>) -> QueryCandidate {
    QueryCandidate::ready(
        repository_id,
        QueryCoverage::new(
            QueryRange::new(100, 200).expect("observed"),
            QueryRange::new(100, 200).expect("received"),
        ),
        vec![StreamWatermark::new("source-a", 1, "traffic", 200).expect("watermark")],
        gaps,
        skew,
    )
    .expect("candidate")
}
