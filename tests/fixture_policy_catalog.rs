#[test]
fn exposes_the_checked_in_synthetic_catalog_to_rust_tests() {
    assert_eq!(xp_test_fixtures::primary_host(), "node-a.fixture.test");
    assert_eq!(xp_test_fixtures::primary_node_id(), "node-fixture-a");
    assert_eq!(
        xp_test_fixtures::primary_endpoint_id(),
        "endpoint-fixture-a"
    );
    assert_eq!(xp_test_fixtures::primary_user_id(), "user-fixture-a");
    assert_eq!(
        xp_test_fixtures::baseline_timestamp(),
        "2024-01-01T00:00:00Z"
    );
    assert_eq!(xp_test_fixtures::low_latency(), 12);
}
