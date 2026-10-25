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

#[test]
fn remaps_references_before_duplicate_detection() {
    let existing = r#"- name: hop
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
"#;
    let source = r#"proxies:
  - name: hop
    type: ss
    server: imported.example
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
"#;
    let preview = preview_text(source, existing, &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 2);
    assert_eq!(preview.counts.skipped, 0);
    assert!(preview.added_proxies_yaml.contains("name: app-2"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop-2"));
}

#[test]
fn preserves_forward_dialer_reference_before_duplicate_detection() {
    let existing = r#"- name: app
  type: ss
  server: app.example
  port: 443
  cipher: aes-256-gcm
  password: secret
  dialer-proxy: hop
- name: hop
  type: ss
  server: hop.example
  port: 443
  cipher: aes-256-gcm
  password: secret
"#;
    let source = r#"proxies:
  - name: app
    type: ss
    server: app.example
    port: 443
    cipher: aes-256-gcm
    password: secret
    dialer-proxy: hop
  - name: hop
    type: ss
    server: imported.example
    port: 443
    cipher: aes-256-gcm
    password: secret
"#;
    let preview = preview_text(source, existing, &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 2);
    assert_eq!(preview.counts.skipped, 0);
    assert!(preview.added_proxies_yaml.contains("name: app-2"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop-2"));
}

#[test]
fn dedupes_after_interleaved_forward_dialer_references_resolve() {
    let existing = r#"- name: hop
  type: socks5
  server: old.example
  port: 1080
"#;
    let source = r#"proxies:
  - name: app-new
    type: socks5
    server: app.example
    port: 1080
    dialer-proxy: hop
  - name: alias
    type: socks5
    server: old.example
    port: 1080
  - name: app-old
    type: socks5
    server: app.example
    port: 1080
    dialer-proxy: alias
  - name: hop
    type: socks5
    server: new.example
    port: 1080
"#;
    let preview = preview_text(source, existing, &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 3);
    assert_eq!(preview.counts.skipped, 1);
    assert!(preview.added_proxies_yaml.contains("name: app-new"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop-2"));
    assert!(preview.added_proxies_yaml.contains("name: app-old"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop\n"));
}

#[test]
fn removes_ambiguous_same_name_self_reference() {
    let source = r#"proxies:
  - name: hop
    type: ss
    server: first.example
    port: 443
    cipher: aes-256-gcm
    password: secret
  - name: hop
    type: ss
    server: second.example
    port: 443
    cipher: aes-256-gcm
    password: secret
    dialer-proxy: hop
"#;
    let preview = preview_text(source, "", &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 2);
    assert!(preview.added_proxies_yaml.contains("name: hop-2"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop\n"));
    assert!(!preview.added_proxies_yaml.contains("dialer-proxy: hop-2"));
}

#[test]
fn remaps_trimmed_source_names() {
    let source = r#"proxies:
  - name: " hop "
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
    dialer-proxy: " hop "
"#;
    let preview = preview_text(source, "", &BTreeSet::new()).expect("preview");
    assert_eq!(preview.counts.added, 2);
    assert!(preview.added_proxies_yaml.contains("name: hop\n"));
    assert!(preview.added_proxies_yaml.contains("dialer-proxy: hop"));
}

#[test]
fn sanitizes_sensitive_report_fields() {
    let source = r#"proxies:
  - name: "https://provider.example/sub?token=SECRET"
    type: "https://type.example?token=SECRET"
    server: proxy.example
    port: 443
"#;
    let preview = preview_text(source, "", &BTreeSet::new()).expect("preview");
    let item = preview.items.first().expect("item");
    assert_eq!(item.name.as_deref(), Some("imported-001"));
    assert_eq!(item.scheme.as_deref(), Some("unknown"));
}

#[test]
fn sanitizes_raw_failure_scheme() {
    let preview = preview_text("user:secret@host://bad", "", &BTreeSet::new()).expect("preview");
    let item = preview.items.first().expect("item");
    assert_eq!(item.scheme, None);
}
