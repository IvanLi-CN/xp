#[test]
fn remaps_dialer_proxy_after_name_collision_and_duplicate_skip() {
    let existing = r#"- name: hop
  type: ss
  server: hop.example
  port: 443
  cipher: aes-256-gcm
  password: secret
"#;
    let source = r#"proxies:
  - name: hop
    type: ss
    server: imported.example
    port: 443
    cipher: aes-256-gcm
    password: secret
  - name: alias
    type: ss
    server: hop.example
    port: 443
    cipher: aes-256-gcm
    password: secret
  - name: app
    type: ss
    server: app.example
    port: 443
    cipher: aes-256-gcm
    password: secret
    dialer-proxy: hop
  - name: app-via-existing
    type: ss
    server: app-2.example
    port: 443
    cipher: aes-256-gcm
    password: secret
    dialer-proxy: alias
"#;
    let preview = preview_text(source, existing, &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 3);
    assert_eq!(preview.counts.skipped, 1);
    assert!(preview.added_proxies_yaml.contains("name: hop-2"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop-2"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop\n"));
}
