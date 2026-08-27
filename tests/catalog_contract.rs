use kie_mcp::kie::catalog::{CATALOG_SOURCE, model_catalog};
use serde_json::Value;

#[test]
fn embedded_catalog_matches_reviewed_kie_contract_snapshot() {
    let snapshot: Value = serde_json::from_str(include_str!("fixtures/kie_catalog_contract.json"))
        .expect("catalog contract snapshot should be valid JSON");
    let actual =
        serde_json::to_value(model_catalog()).expect("embedded catalog should serialize to JSON");

    assert_eq!(snapshot["source"], CATALOG_SOURCE);
    assert_eq!(snapshot["verified_at"], "2026-08-28");
    assert_eq!(
        snapshot["llms_sha256"],
        "1f2e479b07265e9bd5b422e00009aa929fe6a8b5f1ff8c74a02a1cd56668bd01"
    );
    assert_eq!(snapshot["models"].as_array().unwrap().len(), 128);
    assert_eq!(
        snapshot["models"], actual,
        "catalog metadata changed: review the corresponding KIE model pages before updating \
         tests/fixtures/kie_catalog_contract.json"
    );
}
