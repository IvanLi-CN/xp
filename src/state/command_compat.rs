use std::collections::BTreeSet;

use super::{DesiredStateCommand, DesiredStateCommandCompat};

impl From<DesiredStateCommandCompat> for DesiredStateCommand {
    fn from(value: DesiredStateCommandCompat) -> Self {
        match value {
            DesiredStateCommandCompat::UpsertNode { node, join_session } => {
                Self::UpsertNode { node, join_session }
            }
            DesiredStateCommandCompat::DeleteNode {
                node_id,
                delete_endpoints,
                expected_endpoint_ids,
                join_session,
            } => Self::DeleteNode {
                node_id,
                delete_endpoints,
                expected_endpoint_ids,
                join_session,
            },
            DesiredStateCommandCompat::BeginMembershipOperation {
                operation,
                node,
                join_session,
            } => Self::BeginMembershipOperation {
                operation,
                node,
                join_session,
            },
            DesiredStateCommandCompat::TransitionMembershipOperation { operation } => {
                Self::TransitionMembershipOperation { operation }
            }
            DesiredStateCommandCompat::PruneMembershipOperations { terminal_before } => {
                Self::PruneMembershipOperations { terminal_before }
            }
            DesiredStateCommandCompat::UpsertEndpoint { endpoint, expected } => {
                Self::UpsertEndpoint { endpoint, expected }
            }
            DesiredStateCommandCompat::DeleteEndpoint { endpoint_id } => {
                Self::DeleteEndpoint { endpoint_id }
            }
            DesiredStateCommandCompat::CreateServiceMonitor { monitor } => {
                Self::CreateServiceMonitor { monitor }
            }
            DesiredStateCommandCompat::UpdateServiceMonitor {
                monitor,
                expected_revision,
            } => Self::UpdateServiceMonitor {
                monitor,
                expected_revision,
            },
            DesiredStateCommandCompat::SetServiceMonitorLifecycle {
                monitor_id,
                lifecycle,
                expected_revision,
                revision_effective_at_unix_seconds,
            } => Self::SetServiceMonitorLifecycle {
                monitor_id,
                lifecycle,
                expected_revision,
                revision_effective_at_unix_seconds,
            },
            DesiredStateCommandCompat::DeleteServiceMonitor {
                monitor_id,
                expected_revision,
            } => Self::DeleteServiceMonitor {
                monitor_id,
                expected_revision,
            },
            DesiredStateCommandCompat::CreateRealityDomain { domain } => {
                Self::CreateRealityDomain { domain }
            }
            DesiredStateCommandCompat::PatchRealityDomain {
                domain_id,
                server_name,
                disabled_node_ids,
            } => Self::PatchRealityDomain {
                domain_id,
                server_name,
                disabled_node_ids,
            },
            DesiredStateCommandCompat::DeleteRealityDomain { domain_id } => {
                Self::DeleteRealityDomain { domain_id }
            }
            DesiredStateCommandCompat::ReorderRealityDomains { domain_ids } => {
                Self::ReorderRealityDomains { domain_ids }
            }
            DesiredStateCommandCompat::UpsertUser { user } => Self::UpsertUser { user },
            DesiredStateCommandCompat::DeleteUser { user_id } => Self::DeleteUser { user_id },
            DesiredStateCommandCompat::ResetUserSubscriptionToken {
                user_id,
                subscription_token,
            } => Self::ResetUserSubscriptionToken {
                user_id,
                subscription_token,
            },
            DesiredStateCommandCompat::SetUserNodeQuota {
                user_id,
                node_id,
                quota_limit_bytes,
                quota_reset_source,
            } => Self::SetUserNodeQuota {
                user_id,
                node_id,
                quota_limit_bytes,
                quota_reset_source,
            },
            DesiredStateCommandCompat::SetUserNodeWeight {
                user_id,
                node_id,
                weight,
            } => Self::SetUserNodeWeight {
                user_id,
                node_id,
                weight,
            },
            DesiredStateCommandCompat::SetUserGlobalWeight { user_id, weight } => {
                Self::SetUserGlobalWeight { user_id, weight }
            }
            DesiredStateCommandCompat::SetNodeWeightPolicy {
                node_id,
                inherit_global,
            } => Self::SetNodeWeightPolicy {
                node_id,
                inherit_global,
            },
            DesiredStateCommandCompat::SetUserMihomoProfile { user_id, profile } => {
                Self::SetUserMihomoProfile { user_id, profile }
            }
            DesiredStateCommandCompat::SetMihomoDeliveryMode { mode } => {
                Self::SetMihomoDeliveryMode { mode }
            }
            DesiredStateCommandCompat::SetMihomoResourceAllowPrivateTargets { allow } => {
                Self::SetMihomoResourceAllowPrivateTargets { allow }
            }
            DesiredStateCommandCompat::SetGeoDbUpdateSettings { settings } => Self::CompatNoop {
                note: format!(
                    "ignored legacy geo_db settings command: {}",
                    settings.provider
                ),
            },
            DesiredStateCommandCompat::ReplaceUserAccess {
                user_id,
                endpoint_ids,
                items,
            } => {
                let mut merged: BTreeSet<String> = endpoint_ids.into_iter().collect();
                merged.extend(items.into_iter().map(|i| i.endpoint_id));
                Self::ReplaceUserAccess {
                    user_id,
                    endpoint_ids: merged.into_iter().collect(),
                }
            }
            DesiredStateCommandCompat::EnsureMembership {
                user_id,
                endpoint_id,
            } => Self::EnsureMembership {
                user_id,
                endpoint_id,
            },
            DesiredStateCommandCompat::BumpUserCredentialEpoch { user_id } => {
                Self::BumpUserCredentialEpoch { user_id }
            }
            DesiredStateCommandCompat::CompatNoop { note } => Self::CompatNoop { note },
            DesiredStateCommandCompat::AppendEndpointProbeSamples {
                hour,
                from_node_id,
                samples,
            } => Self::AppendEndpointProbeSamples {
                hour,
                from_node_id,
                samples,
            },
            DesiredStateCommandCompat::ReplaceRepositoryMembership { membership } => {
                Self::ReplaceRepositoryMembership { membership }
            }
            DesiredStateCommandCompat::UpdateRepositoryMemberRuntime(patch) => {
                Self::UpdateRepositoryMemberRuntime(patch)
            }
            DesiredStateCommandCompat::SetResourcePolicy {
                policy,
                expected_revision,
            } => Self::SetResourcePolicy {
                policy,
                expected_revision,
            },
            DesiredStateCommandCompat::SetReverseMeshEpoch { epoch } => {
                Self::SetReverseMeshEpoch { epoch }
            }
            DesiredStateCommandCompat::UpsertReverseMeshAssignment {
                assignment,
                expected_generation,
            } => Self::UpsertReverseMeshAssignment {
                assignment,
                expected_generation,
            },
            DesiredStateCommandCompat::DeleteReverseMeshAssignment {
                target_node_id,
                expected_generation,
            } => Self::DeleteReverseMeshAssignment {
                target_node_id,
                expected_generation,
            },
            DesiredStateCommandCompat::ReplaceUserGrants { user_id, grants } => {
                let endpoint_ids = grants.into_iter().map(|g| g.endpoint_id).collect();
                Self::ReplaceUserAccess {
                    user_id,
                    endpoint_ids,
                }
            }
            DesiredStateCommandCompat::UpsertGrant { grant } => Self::EnsureMembership {
                user_id: grant.user_id,
                endpoint_id: grant.endpoint_id,
            },
            DesiredStateCommandCompat::DeleteGrant { grant_id } => Self::CompatNoop {
                note: format!("legacy delete_grant ignored: {grant_id}"),
            },
            DesiredStateCommandCompat::SetGrantEnabled {
                grant_id,
                enabled,
                source: _,
            } => Self::CompatNoop {
                note: format!(
                    "legacy set_grant_enabled ignored: grant_id={grant_id} enabled={enabled}"
                ),
            },
        }
    }
}
