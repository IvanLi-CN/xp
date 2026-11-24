//! Deploy-time ingress runtime selection and activation plan tests.

use super::activation::{ManagedServiceActivationStep, managed_service_activation_steps};
use super::*;
use crate::ops::runtime::RuntimeTarget;
use crate::ops::runtime_boot::BootStep;
use tempfile::tempdir;

#[test]
fn post_join_service_activation_orders_and_waits_each_service() {
    assert_eq!(
        managed_service_activation_steps(RuntimeTarget::Xray, true, false),
        vec![
            ManagedServiceActivationStep::Boot(BootStep::Disable("xp-proxy")),
            ManagedServiceActivationStep::Boot(BootStep::Enable("xray")),
            ManagedServiceActivationStep::StartOrRestart("xray"),
            ManagedServiceActivationStep::StartOrRestart("xp"),
            ManagedServiceActivationStep::WaitReady("xray"),
            ManagedServiceActivationStep::WaitReady("xp"),
        ]
    );
    assert_eq!(
        managed_service_activation_steps(RuntimeTarget::XpProxy, true, false),
        vec![
            ManagedServiceActivationStep::Boot(BootStep::Disable("xray")),
            ManagedServiceActivationStep::Boot(BootStep::Enable("xp-proxy")),
            ManagedServiceActivationStep::StartOrRestart("xp-proxy"),
            ManagedServiceActivationStep::StartOrRestart("xp"),
            ManagedServiceActivationStep::WaitReady("xp-proxy"),
            ManagedServiceActivationStep::WaitReady("xp"),
        ]
    );
}

#[test]
fn deploy_activation_keeps_cloudflared_boot_enablement_and_readiness() {
    assert_eq!(
        managed_service_activation_steps(RuntimeTarget::XpProxy, false, true),
        vec![
            ManagedServiceActivationStep::Boot(BootStep::Enable("xp-proxy")),
            ManagedServiceActivationStep::StartOrRestart("xp-proxy"),
            ManagedServiceActivationStep::StartOrRestart("xp"),
            ManagedServiceActivationStep::WaitReady("xp-proxy"),
            ManagedServiceActivationStep::WaitReady("xp"),
            ManagedServiceActivationStep::Boot(BootStep::Enable("cloudflared")),
            ManagedServiceActivationStep::StartOrRestart("cloudflared"),
            ManagedServiceActivationStep::WaitReady("cloudflared"),
        ]
    );
}

fn no_cloudflare_deploy_args(xp_bin: PathBuf) -> DeployArgs {
    DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle {
            cloudflare: false,
            no_cloudflare: true,
        },
        ddns_toggle: crate::ops::cli::DdnsToggle::default(),
        ip_geo_enabled: false,
        account_id: None,
        zone_id: None,
        hostname: xp_test_fixtures::none(),
        tunnel_name: None,
        origin_url: None,
        migrate_existing_tunnel: false,
        ddns_zone_id: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        join_token: None,
        join_token_stdin: false,
        join_token_stdin_value: None,
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: Some(xp_test_fixtures::service_fixture564().to_owned()),
        xray_version: "latest".to_string(),
        runtime: crate::ops::cli::DeployRuntimeArg::Auto,
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: true,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    }
}

fn deploy_fixture(xp_bin: PathBuf) -> (tempfile::TempDir, Paths, DeployArgs) {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::write(&xp_bin, b"dummy").unwrap();
    let args = no_cloudflare_deploy_args(xp_bin);
    (tmp, paths, args)
}

#[tokio::test]
async fn build_plan_defaults_a_fresh_deployment_to_xp_proxy() {
    let tmp = tempdir().unwrap();
    let xp_bin = tmp.path().join("xp");
    let (_tmp, paths, args) = deploy_fixture(xp_bin);

    let plan = build_plan(&paths, &args).await.unwrap();

    assert_eq!(plan.runtime_selection.target, RuntimeTarget::XpProxy);
    assert_eq!(
        plan.runtime_selection.source,
        crate::ops::runtime_selection::RuntimeSelectionSource::FreshDefault
    );
}

#[tokio::test]
async fn build_plan_allows_explicit_xray_on_a_fresh_deployment() {
    let tmp = tempdir().unwrap();
    let xp_bin = tmp.path().join("xp");
    let (_tmp, paths, mut args) = deploy_fixture(xp_bin);
    args.runtime = crate::ops::cli::DeployRuntimeArg::Xray;

    let plan = build_plan(&paths, &args).await.unwrap();

    assert_eq!(plan.runtime_selection.target, RuntimeTarget::Xray);
    assert_eq!(
        plan.runtime_selection.source,
        crate::ops::runtime_selection::RuntimeSelectionSource::Explicit
    );
}

#[tokio::test]
async fn build_plan_keeps_legacy_xray_on_an_existing_deployment() {
    let tmp = tempdir().unwrap();
    let xp_bin = tmp.path().join("xp");
    let (_tmp, paths, args) = deploy_fixture(xp_bin);
    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(paths.etc_xp_env(), "XP_DATA_DIR=/var/lib/xp/data\n").unwrap();

    let plan = build_plan(&paths, &args).await.unwrap();

    assert_eq!(plan.runtime_selection.target, RuntimeTarget::Xray);
    assert_eq!(
        plan.runtime_selection.source,
        crate::ops::runtime_selection::RuntimeSelectionSource::ExistingDeploymentLegacyXray
    );
}

#[tokio::test]
async fn build_plan_retry_after_a_failed_fresh_deploy_stays_xp_proxy() {
    let tmp = tempdir().unwrap();
    let xp_bin = tmp.path().join("xp");
    let (_tmp, paths, args) = deploy_fixture(xp_bin);
    // A failed deploy already wrote the durable selection and xp.env before activation.
    crate::ops::runtime_selection::persist_deploy_selection(&paths, RuntimeTarget::XpProxy)
        .unwrap();
    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(paths.etc_xp_env(), "XP_DATA_DIR=/var/lib/xp/data\n").unwrap();

    let plan = build_plan(&paths, &args).await.unwrap();

    assert_eq!(plan.runtime_selection.target, RuntimeTarget::XpProxy);
    assert_eq!(
        plan.runtime_selection.source,
        crate::ops::runtime_selection::RuntimeSelectionSource::PersistedDeploySelection
    );
}

#[tokio::test]
async fn build_plan_rejects_a_conflicting_runtime_on_an_existing_node() {
    let tmp = tempdir().unwrap();
    let xp_bin = tmp.path().join("xp");
    let (_tmp, paths, mut args) = deploy_fixture(xp_bin);
    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(paths.etc_xp_env(), "XP_DATA_DIR=/var/lib/xp/data\n").unwrap();
    args.runtime = crate::ops::cli::DeployRuntimeArg::XpProxy;

    let error = build_plan(&paths, &args)
        .await
        .expect_err("an existing legacy node must not switch implicitly");

    assert_eq!(error.code, 2);
    assert!(error.message.starts_with("runtime_conflict:"));
    assert!(
        error
            .message
            .contains("xp-ops runtime switch --to xp-proxy --yes")
    );
}
