//! Serial Xray dynamic configuration reconciler for XP-owned reverse handlers.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use prost::Message;

use crate::xray::proto::xray as xproto;
use crate::{
    domain::{Endpoint, Node},
    managed_default_endpoints::managed_default_vless_endpoint,
    protocol::{
        RealityConfig, RealityKeys, VlessRealityTransport, VlessRealityVisionTcpEndpointMeta,
    },
    reverse_mesh::{
        REVERSE_PORTAL_ADDRESS, REVERSE_SOCKS_USERNAME, ReverseLinkKey, ReverseMeshAssignment,
        ReverseMeshBootstrapMarker, ReverseRole, ReverseTombstones, TombstoneDecision,
        derive_reverse_origin, derive_reverse_origin_id, derive_reverse_password,
        derive_reverse_tag, derive_reverse_uuid,
    },
    xray::builder::ReverseVlessEndpoint,
    xray::{self, builder},
};

#[derive(Debug, Clone)]
pub struct ReversePortalSpec {
    pub tag: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct ReverseInboundUserOperation {
    pub inbound_tag: String,
    pub operation: xproto::common::serial::TypedMessage,
}

#[derive(Debug, Clone, Default)]
pub struct ReverseXrayDesired {
    pub portal: Option<ReversePortalSpec>,
    pub outbound_requests: Vec<xproto::app::proxyman::command::AddOutboundRequest>,
    pub inbound_user_operations: Vec<ReverseInboundUserOperation>,
    pub route_requests: Vec<xproto::app::router::command::AddRuleRequest>,
    pub owned_inbound_tags: BTreeSet<String>,
    pub owned_outbound_tags: BTreeSet<String>,
    pub owned_rule_tags: BTreeSet<String>,
    pub active_target_links: BTreeSet<ReverseLinkKey>,
    pub healthy_target_links: BTreeSet<ReverseLinkKey>,
    pub target_links_present: bool,
    pub fail_closed_outbound_tags: BTreeSet<String>,
    pub managed_vless_inbound_tags: BTreeSet<String>,
    pub owned_inbound_user_emails: BTreeMap<String, BTreeSet<String>>,
}

/// Builds the complete XP-owned Xray delta for one local node. It is pure with respect to Raft
/// state so the desired set can be unit-tested before a live HandlerService is contacted.
#[allow(clippy::too_many_arguments)]
pub fn build_reverse_desired(
    local_node_id: &str,
    cluster_ca_key_pem: &str,
    epoch: u64,
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    nodes: &[Node],
    endpoints: &[Endpoint],
    local_xp_port: u16,
    bootstrap: Option<&ReverseMeshBootstrapMarker>,
    bootstrap_target_node_id: Option<&str>,
    enabled_target_links: &BTreeSet<ReverseLinkKey>,
    healthy_target_links: &BTreeSet<ReverseLinkKey>,
    target_links_present: bool,
) -> Result<ReverseXrayDesired, String> {
    // A freshly joined node receives the marker before its first Raft snapshot. Its local
    // desired state therefore still has epoch zero, but the marker already carries the epoch
    // that was reserved by the leader. Re-enter with that epoch so all derived tags, UUIDs and
    // origins stay identical to the leader's assignment.
    if epoch == 0
        && let Some(marker) =
            bootstrap.filter(|marker| marker.target_node_id == local_node_id && marker.epoch > 0)
    {
        return build_reverse_desired(
            local_node_id,
            cluster_ca_key_pem,
            marker.epoch,
            assignments,
            nodes,
            endpoints,
            local_xp_port,
            Some(marker),
            bootstrap_target_node_id,
            enabled_target_links,
            healthy_target_links,
            target_links_present,
        );
    }
    let mut effective_assignments = assignments.clone();
    let mut effective_nodes = nodes.to_vec();
    let mut effective_endpoints = endpoints.to_vec();
    if let Some(marker) =
        bootstrap.filter(|marker| marker.epoch == epoch && marker.target_node_id == local_node_id)
    {
        effective_assignments
            .entry(marker.target_node_id.clone())
            .or_insert_with(|| ReverseMeshAssignment {
                target_node_id: marker.target_node_id.clone(),
                generation: marker.generation.max(1),
                membership_revision: 1,
                primary_node_id: marker.primary_node_id.clone(),
                standby_node_id: marker.standby_node_id.clone(),
                credential_epoch: marker.epoch,
            });
        let mut bootstrap_endpoints = vec![(&marker.primary_node_id, &marker.primary_endpoint)];
        if let (Some(rendezvous_id), Some(endpoint)) = (
            marker.standby_node_id.as_ref(),
            marker.standby_endpoint.as_ref(),
        ) {
            bootstrap_endpoints.push((rendezvous_id, endpoint));
        }
        for (rendezvous_id, endpoint) in bootstrap_endpoints {
            if rendezvous_id.is_empty()
                || effective_nodes
                    .iter()
                    .any(|node| node.node_id == *rendezvous_id)
            {
                continue;
            }
            effective_nodes.push(Node {
                node_id: rendezvous_id.to_string(),
                node_name: format!("reverse-bootstrap-{rendezvous_id}"),
                access_host: endpoint.access_host.clone(),
                api_base_url: format!("https://{}", endpoint.access_host),
                quota_limit_bytes: 0,
                quota_reset: Default::default(),
            });
            effective_endpoints.push(Endpoint {
                endpoint_id: format!("reverse-bootstrap-{rendezvous_id}"),
                node_id: rendezvous_id.to_string(),
                tag: format!("reverse-bootstrap-vless-{rendezvous_id}"),
                kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
                port: endpoint.port,
                meta: serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
                    reality: RealityConfig {
                        dest: String::new(),
                        server_names: vec![endpoint.server_name.clone()],
                        server_names_source: Default::default(),
                        fingerprint: "chrome".to_string(),
                    },
                    reality_keys: RealityKeys {
                        private_key: String::new(),
                        public_key: endpoint.public_key.clone(),
                    },
                    short_ids: vec![endpoint.short_id.clone()],
                    active_short_id: endpoint.short_id.clone(),
                    canary_upstream: None,
                    accepted_authorities: Vec::new(),
                    mihomo_smux: Default::default(),
                    transport: if endpoint.transport == "xhttp" {
                        VlessRealityTransport::Xhttp
                    } else {
                        VlessRealityTransport::VisionTcp
                    },
                    managed_default: true,
                })
                .map_err(|error| format!("encode reverse bootstrap endpoint: {error}"))?,
            });
        }
    }
    let mut desired = ReverseXrayDesired {
        target_links_present,
        ..Default::default()
    };
    let local_vless = effective_endpoints
        .iter()
        .filter(|endpoint| endpoint.node_id == local_node_id)
        .filter(|endpoint| managed_default_vless_endpoint(endpoint).is_some())
        .cloned()
        .collect::<Vec<_>>();
    desired.managed_vless_inbound_tags = local_vless
        .iter()
        .map(|endpoint| endpoint.tag.clone())
        .collect();
    let mut exact_rules = Vec::new();
    let mut block_rules = Vec::new();

    for assignment in effective_assignments.values() {
        if assignment.credential_epoch != epoch || !assignment.is_valid() {
            continue;
        }
        if assignment.contains_rendezvous(local_node_id) {
            let role = crate::reverse_mesh::reverse_assignment_role(
                assignment,
                local_node_id,
                bootstrap_target_node_id.is_some_and(|target| target == assignment.target_node_id),
            )
            .ok_or_else(|| format!("reverse node {local_node_id} is not an assigned Rendezvous"))?;
            let portal_tag = format!("xp-reverse-portal-{epoch}-{local_node_id}");
            desired.portal = Some(ReversePortalSpec {
                tag: portal_tag.clone(),
                username: REVERSE_SOCKS_USERNAME.to_string(),
                password: derive_reverse_password(cluster_ca_key_pem, local_node_id, epoch),
            });
            desired.owned_inbound_tags.insert(portal_tag.clone());
            let reverse_tag = derive_reverse_tag(
                epoch,
                &assignment.target_node_id,
                local_node_id,
                role,
                assignment.generation,
            );
            desired.owned_inbound_tags.insert(reverse_tag.clone());
            let origin = derive_reverse_origin(&derive_reverse_origin_id(
                epoch,
                &assignment.target_node_id,
                local_node_id,
                role,
                assignment.generation,
            ));
            let exact_rule_tag = format!(
                "xp-reverse-route-{epoch}-{}-{}-{}",
                assignment.target_node_id,
                role.as_str(),
                assignment.generation
            );
            exact_rules.push(builder::build_reverse_route_rule(
                &exact_rule_tag,
                &portal_tag,
                &origin,
                &reverse_tag,
            ));
            desired.owned_rule_tags.insert(exact_rule_tag);
            let block_rule_tag = format!(
                "xp-reverse-block-{epoch}-{}-{}-{}",
                assignment.target_node_id,
                role.as_str(),
                assignment.generation
            );
            block_rules.push(builder::build_reverse_block_rule(
                &block_rule_tag,
                &portal_tag,
                "block",
            ));
            desired.owned_rule_tags.insert(block_rule_tag);

            for endpoint in &local_vless {
                let email = format!(
                    "reverse:{}:{}:{}",
                    assignment.target_node_id,
                    assignment.generation,
                    role.as_str()
                );
                let uuid = derive_reverse_uuid(
                    cluster_ca_key_pem,
                    epoch,
                    &assignment.target_node_id,
                    local_node_id,
                    role,
                    assignment.generation,
                );
                let operation = builder::build_reverse_add_user_operation(
                    endpoint,
                    &email,
                    &uuid,
                    &reverse_tag,
                )
                .map_err(|error| error.to_string())?;
                desired
                    .inbound_user_operations
                    .push(ReverseInboundUserOperation {
                        inbound_tag: endpoint.tag.clone(),
                        operation,
                    });
                desired
                    .owned_inbound_user_emails
                    .entry(endpoint.tag.clone())
                    .or_default()
                    .insert(email);
            }
        }

        if assignment.target_node_id == local_node_id {
            for (rendezvous_id, formal_role) in [
                (assignment.primary_node_id.as_str(), ReverseRole::Primary),
                (
                    assignment.standby_node_id.as_deref().unwrap_or_default(),
                    ReverseRole::Standby,
                ),
            ] {
                if rendezvous_id.is_empty() {
                    continue;
                }
                let role = if bootstrap.is_some() || bootstrap_target_node_id == Some(local_node_id)
                {
                    ReverseRole::Bootstrap
                } else {
                    formal_role
                };
                let link = ReverseLinkKey::new(
                    epoch,
                    local_node_id,
                    rendezvous_id,
                    role,
                    assignment.generation,
                );
                let outbound_tag = format!(
                    "xp-reverse-outbound-{epoch}-{rendezvous_id}-{role:?}-{}",
                    assignment.generation
                );
                let freedom_tag = format!(
                    "xp-reverse-freedom-{epoch}-{rendezvous_id}-{role:?}-{}",
                    assignment.generation
                );
                if !enabled_target_links.contains(&link) {
                    desired.fail_closed_outbound_tags.insert(outbound_tag);
                    desired.fail_closed_outbound_tags.insert(freedom_tag);
                    continue;
                }
                desired.active_target_links.insert(link.clone());
                if healthy_target_links.contains(&link) {
                    desired.healthy_target_links.insert(link.clone());
                }
                let rendezvous = effective_nodes
                    .iter()
                    .find(|node| node.node_id == rendezvous_id)
                    .ok_or_else(|| format!("reverse Rendezvous {rendezvous_id} is missing"))?;
                let endpoint = effective_endpoints
                    .iter()
                    .find(|endpoint| {
                        endpoint.node_id == rendezvous_id
                            && managed_default_vless_endpoint(endpoint).is_some()
                    })
                    .ok_or_else(|| {
                        format!("reverse Rendezvous {rendezvous_id} has no managed VLESS endpoint")
                    })?;
                let meta = managed_default_vless_endpoint(endpoint)
                    .ok_or_else(|| "reverse endpoint metadata is invalid".to_string())?;
                let server_name = meta.reality.server_names.first().cloned().ok_or_else(|| {
                    format!("reverse Rendezvous {rendezvous_id} has no Reality server name")
                })?;
                let reverse_tag = derive_reverse_tag(
                    epoch,
                    local_node_id,
                    rendezvous_id,
                    role,
                    assignment.generation,
                );
                let uuid = derive_reverse_uuid(
                    cluster_ca_key_pem,
                    epoch,
                    local_node_id,
                    rendezvous_id,
                    role,
                    assignment.generation,
                );
                let reverse_endpoint = ReverseVlessEndpoint {
                    access_host: rendezvous.access_host.clone(),
                    endpoint: endpoint.clone(),
                    target_port: endpoint.port,
                    target_public_key_b64url_nopad: meta.reality_keys.public_key.clone(),
                    target_short_id_hex: meta.active_short_id.clone(),
                    server_name,
                };
                desired.outbound_requests.push(
                    builder::build_reverse_vless_outbound_request(
                        &outbound_tag,
                        &reverse_tag,
                        &uuid,
                        &reverse_endpoint,
                    )
                    .map_err(|error| error.to_string())?,
                );
                desired
                    .outbound_requests
                    .push(builder::build_reverse_freedom_outbound_request(
                        &freedom_tag,
                        "127.0.0.1",
                        local_xp_port,
                    ));
                desired.owned_outbound_tags.insert(outbound_tag.clone());
                desired.owned_outbound_tags.insert(freedom_tag.clone());
                desired.owned_inbound_tags.insert(reverse_tag.clone());
                let origin = derive_reverse_origin(&derive_reverse_origin_id(
                    epoch,
                    local_node_id,
                    rendezvous_id,
                    role,
                    assignment.generation,
                ));
                let route_tag = format!(
                    "xp-reverse-target-route-{epoch}-{rendezvous_id}-{role:?}-{}",
                    assignment.generation
                );
                exact_rules.push(builder::build_reverse_route_rule(
                    &route_tag,
                    &reverse_tag,
                    &origin,
                    &freedom_tag,
                ));
                desired.owned_rule_tags.insert(route_tag);
                let block_tag = format!(
                    "xp-reverse-target-block-{epoch}-{rendezvous_id}-{role:?}-{}",
                    assignment.generation
                );
                block_rules.push(builder::build_reverse_block_rule(
                    &block_tag,
                    &reverse_tag,
                    "block",
                ));
                desired.owned_rule_tags.insert(block_tag);
            }
        }
    }
    desired.route_requests.extend(exact_rules);
    desired.route_requests.extend(block_rules);
    Ok(desired)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseReconcileStatus {
    Active,
    Disabled,
    RestartRequired,
}

#[derive(Debug, Default)]
pub struct ReverseXrayReconciler {
    tombstones: ReverseTombstones,
    configured: bool,
    retiring: BTreeMap<String, Instant>,
}

impl ReverseXrayReconciler {
    pub fn tombstone_count(&self) -> usize {
        self.tombstones.len()
    }

    pub fn reset_after_restart(&mut self) {
        self.tombstones.clear_after_restart();
    }

    pub fn has_managed_state(&self) -> bool {
        self.configured
    }

    fn should_remove_retired(&mut self, tag: &str, now: Instant) -> bool {
        let deadline = self
            .retiring
            .entry(tag.to_string())
            .or_insert_with(|| now + Duration::from_secs(120));
        if now >= *deadline {
            self.retiring.remove(tag);
            true
        } else {
            false
        }
    }

    /// Reconciles XP-owned rules serially. A non-XP inbound on the fixed portal port is a hard
    /// conflict; the reconciler never moves the portal to a new public or random port.
    pub async fn reconcile(
        &mut self,
        client: &mut xray::XrayClient,
        desired: &ReverseXrayDesired,
        remove_immediately: bool,
    ) -> Result<ReverseReconcileStatus, tonic::Status> {
        self.configured = true;
        let now = Instant::now();
        // Keep only stale handlers while they drain. A desired tag must not retain an old
        // retirement deadline, while an absent tag must keep its original deadline across
        // reconciliation passes so the 120-second drain is real rather than perpetually renewed.
        self.retiring.retain(|tag, _| {
            tag.starts_with("user:")
                || (!desired.owned_outbound_tags.contains(tag)
                    && !desired.owned_inbound_tags.contains(tag))
        });
        if let Some(portal) = desired.portal.as_ref() {
            self.ensure_portal_port(client, &portal.tag).await?;
        }

        let mut restart_required = false;
        let existing_outbounds = client.list_outbounds().await?.outbounds;
        let existing_outbound_by_tag = existing_outbounds
            .iter()
            .map(|outbound| (outbound.tag.clone(), outbound.clone()))
            .collect::<BTreeMap<_, _>>();
        for outbound in existing_outbounds {
            let tag = outbound.tag;
            if tag.starts_with("xp-reverse-") && !desired.owned_outbound_tags.contains(&tag) {
                let defer_replacement_drain =
                    desired.target_links_present && desired.healthy_target_links.is_empty();
                if !remove_immediately
                    && !desired.fail_closed_outbound_tags.contains(&tag)
                    && (defer_replacement_drain || !self.should_remove_retired(&tag, now))
                {
                    continue;
                }
                match client
                    .remove_outbound(xproto::app::proxyman::command::RemoveOutboundRequest {
                        tag: tag.clone(),
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(_) => {
                        restart_required |=
                            self.tombstones.record(tag) == TombstoneDecision::RestartRequired;
                    }
                }
            }
        }

        let existing_rules = client.list_rules().await?;
        let existing_rule_tags = existing_rules
            .rules
            .iter()
            .map(|rule| {
                if rule.rule_tag.is_empty() {
                    rule.tag.clone()
                } else {
                    rule.rule_tag.clone()
                }
            })
            .collect::<BTreeSet<_>>();
        for rule in existing_rules.rules {
            let tag = if rule.rule_tag.is_empty() {
                rule.tag
            } else {
                rule.rule_tag
            };
            if tag.starts_with("xp-reverse-") && !desired.owned_rule_tags.contains(&tag) {
                // Routes are admission controls, not the response stream itself. Remove stale
                // generation rules immediately so an old portal-wide block rule cannot shadow a
                // newly appended exact rule; the old handler/outbound remains for 120 seconds.
                match client
                    .remove_rule(xproto::app::router::command::RemoveRuleRequest {
                        rule_tag: tag.clone(),
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(_) => {
                        restart_required |=
                            self.tombstones.record(tag) == TombstoneDecision::RestartRequired;
                    }
                }
            }
        }

        let existing_inbounds = client.list_inbounds(false).await?;
        for inbound in existing_inbounds.inbounds {
            let tag = inbound.tag;
            if tag.starts_with("xp-reverse-") && !desired.owned_inbound_tags.contains(&tag) {
                if !remove_immediately && !self.should_remove_retired(&tag, now) {
                    continue;
                }
                match client
                    .remove_inbound(xproto::app::proxyman::command::RemoveInboundRequest {
                        tag: tag.clone(),
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(_) => {
                        restart_required |=
                            self.tombstones.record(tag) == TombstoneDecision::RestartRequired;
                    }
                }
            }
        }

        for inbound_tag in &desired.managed_vless_inbound_tags {
            let users = client.get_inbound_users(inbound_tag.clone()).await?;
            let owned = desired
                .owned_inbound_user_emails
                .get(inbound_tag)
                .cloned()
                .unwrap_or_default();
            for user in users.users {
                if !user.email.starts_with("reverse:") || owned.contains(&user.email) {
                    continue;
                }
                let tag = format!("user:{inbound_tag}:{}", user.email);
                if !remove_immediately && !self.should_remove_retired(&tag, now) {
                    continue;
                }
                let operation = builder::build_remove_user_operation(&user.email);
                match client
                    .alter_inbound(xproto::app::proxyman::command::AlterInboundRequest {
                        tag: inbound_tag.clone(),
                        operation: Some(operation),
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(_) => {
                        restart_required |=
                            self.tombstones.record(tag) == TombstoneDecision::RestartRequired;
                    }
                }
            }
        }

        // The order is deliberate: bridge outbounds and users first, portal exact routes next,
        // and the portal block rule last. The fixed loopback portal is created before its rules.
        for request in desired.outbound_requests.iter().cloned() {
            let desired_outbound = request
                .outbound
                .as_ref()
                .ok_or_else(|| tonic::Status::invalid_argument("reverse outbound is missing"))?;
            // HandlerService has no outbound configuration replacement operation. Refresh an
            // XP-owned tag with a changed desired config by removing it before re-adding it.
            if let Some(existing) = existing_outbound_by_tag.get(&desired_outbound.tag)
                && outbound_needs_refresh(existing, &request)?
            {
                match client
                    .remove_outbound(xproto::app::proxyman::command::RemoveOutboundRequest {
                        tag: desired_outbound.tag.clone(),
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(status) if xray::is_not_found(&status) => {}
                    Err(_) => {
                        restart_required |= self.tombstones.record(desired_outbound.tag.clone())
                            == TombstoneDecision::RestartRequired;
                        continue;
                    }
                }
            }
            match client.add_outbound(request).await {
                Ok(_) => {}
                Err(status) if xray::is_already_exists(&status) => {}
                Err(status) => return Err(status),
            }
        }
        for operation in desired.inbound_user_operations.iter() {
            match client
                .alter_inbound(xproto::app::proxyman::command::AlterInboundRequest {
                    tag: operation.inbound_tag.clone(),
                    operation: Some(operation.operation.clone()),
                })
                .await
            {
                Ok(_) => {}
                Err(status) if xray::is_already_exists(&status) => {}
                Err(status) => return Err(status),
            }
        }
        if let Some(portal) = desired.portal.as_ref() {
            let request = builder::build_reverse_socks_inbound_request(
                &portal.tag,
                &portal.username,
                &portal.password,
            );
            match client.add_inbound(request).await {
                Ok(_) => {}
                Err(status) if xray::is_already_exists(&status) => {}
                Err(status) => return Err(status),
            }
        }
        for request in desired.route_requests.iter().cloned() {
            let (rule_tag, already_present) =
                desired_route_presence(&existing_rule_tags, &request)?;
            if already_present {
                continue;
            }
            match client.add_rule(request).await {
                Ok(_) => {}
                Err(status) if xray::is_duplicate_rule_tag(&status, &rule_tag) => {}
                Err(status) => return Err(status),
            }
        }

        if restart_required {
            Ok(ReverseReconcileStatus::RestartRequired)
        } else if desired.portal.is_some() {
            Ok(ReverseReconcileStatus::Active)
        } else {
            Ok(ReverseReconcileStatus::Disabled)
        }
    }

    async fn ensure_portal_port(
        &self,
        client: &mut xray::XrayClient,
        expected_tag: &str,
    ) -> Result<(), tonic::Status> {
        let response = client.list_inbounds(false).await?;
        for inbound in response.inbounds {
            let Some(receiver) = inbound.receiver_settings else {
                continue;
            };
            if receiver.r#type != "xray.app.proxyman.ReceiverConfig" {
                continue;
            }
            let Ok(receiver) =
                xproto::app::proxyman::ReceiverConfig::decode(receiver.value.as_slice())
            else {
                continue;
            };
            let port_conflict = receiver.port_list.is_some_and(|ports| {
                ports
                    .range
                    .iter()
                    .any(|range| range.from <= 10086 && range.to >= 10086)
            });
            if port_conflict && inbound.tag != expected_tag {
                return Err(tonic::Status::failed_precondition(format!(
                    "reverse portal port {REVERSE_PORTAL_ADDRESS} is already owned by another inbound"
                )));
            }
        }
        Ok(())
    }
}

fn requested_rule_tag(
    request: &xproto::app::router::command::AddRuleRequest,
) -> Result<String, tonic::Status> {
    let config = request
        .config
        .as_ref()
        .ok_or_else(|| tonic::Status::invalid_argument("reverse route config is missing"))?;
    let config = xproto::app::router::Config::decode(config.value.as_slice())
        .map_err(|_| tonic::Status::invalid_argument("reverse route config is invalid"))?;
    let [rule] = config.rule.as_slice() else {
        return Err(tonic::Status::invalid_argument(
            "reverse route config must contain exactly one rule",
        ));
    };
    if rule.rule_tag.is_empty() {
        return Err(tonic::Status::invalid_argument(
            "reverse route tag is missing",
        ));
    }
    Ok(rule.rule_tag.clone())
}

fn desired_route_presence(
    existing_rule_tags: &BTreeSet<String>,
    request: &xproto::app::router::command::AddRuleRequest,
) -> Result<(String, bool), tonic::Status> {
    let rule_tag = requested_rule_tag(request)?;
    let already_present = existing_rule_tags.contains(&rule_tag);
    Ok((rule_tag, already_present))
}

fn outbound_needs_refresh(
    existing: &xproto::core::OutboundHandlerConfig,
    desired: &xproto::app::proxyman::command::AddOutboundRequest,
) -> Result<bool, tonic::Status> {
    let desired = desired
        .outbound
        .as_ref()
        .ok_or_else(|| tonic::Status::invalid_argument("reverse outbound is missing"))?;
    Ok(existing != desired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_outbound_is_refreshed_when_its_config_changes() {
        let request = builder::build_reverse_freedom_outbound_request(
            xp_test_fixtures::primary_endpoint_tag(),
            xp_test_fixtures::loopback_address(),
            xp_test_fixtures::number_value1(),
        );
        let existing = request.outbound.clone().expect("outbound is present");
        assert!(!outbound_needs_refresh(&existing, &request).unwrap());

        let mut changed = existing;
        changed.expire = xp_test_fixtures::number_value1();
        assert!(outbound_needs_refresh(&changed, &request).unwrap());
    }

    #[test]
    fn reverse_route_tag_is_decoded_before_idempotent_add() {
        let request = builder::build_reverse_route_rule(
            "xp-reverse-route-7-target-primary-3",
            "xp-reverse-portal-7-rendezvous",
            "rvs-test.mesh.invalid:443",
            "xp-reverse-freedom-7-rendezvous-Primary-3",
        );
        assert_eq!(
            requested_rule_tag(&request).unwrap(),
            "xp-reverse-route-7-target-primary-3"
        );
    }

    #[test]
    fn listed_reverse_route_prevents_another_add() {
        let request = builder::build_reverse_route_rule(
            "xp-reverse-route-7-target-primary-3",
            "xp-reverse-portal-7-rendezvous",
            "rvs-test.mesh.invalid:443",
            "xp-reverse-freedom-7-rendezvous-Primary-3",
        );
        let existing = BTreeSet::from(["xp-reverse-route-7-target-primary-3".to_string()]);

        let (tag, already_present) = desired_route_presence(&existing, &request).unwrap();

        assert_eq!(tag, "xp-reverse-route-7-target-primary-3");
        assert!(already_present);
    }
}
