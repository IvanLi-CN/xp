use base64::Engine as _;

use crate::state::history_repository::identity::{
    Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey,
};

#[test]
fn repository_identity_requires_a_node_id_and_both_public_keys() {
    let node_id = RepositoryNodeId::try_from("node-a".to_owned()).expect("valid node id");
    let identity = RepositoryNodeIdentity::new(
        node_id,
        Ed25519PublicKey::from_bytes([7; 32]).expect("valid signing key"),
        X25519PublicKey::from_bytes([8; 32]).expect("valid relay key"),
    )
    .expect("both keys are present");

    let json = serde_json::to_string(&identity).expect("serialize identity");
    let decoded: RepositoryNodeIdentity =
        serde_json::from_str(&json).expect("deserialize identity");
    assert_eq!(decoded, identity);
    let invalid_encoded_keys = serde_json::json!({
        "node_id": "node-a",
        "ed25519_public_key": "AA",
        "x25519_relay_public_key": "AA",
    });
    assert!(serde_json::from_value::<RepositoryNodeIdentity>(invalid_encoded_keys).is_err());
    assert!(RepositoryNodeId::try_from("  ".to_owned()).is_err());
}

#[test]
fn identity_rejects_missing_or_invalid_public_keys() {
    let missing_relay_key = serde_json::json!({
        "node_id": "node-a",
        "ed25519_public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 32]),
    });
    assert!(serde_json::from_value::<RepositoryNodeIdentity>(missing_relay_key).is_err());
    assert!(Ed25519PublicKey::from_bytes([0; 32]).is_err());
    assert!(X25519PublicKey::from_bytes([0; 32]).is_err());
}
