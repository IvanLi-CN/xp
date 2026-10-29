import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect } from "react";

import { fetchAdminNodeVersions } from "@/api/adminNodeVersions";
import { fetchAdminNodes } from "@/api/adminNodes";
import type { RuntimePolicy } from "@/api/runtimePolicy";
import { readAppliedRuntimePolicyId } from "@/offline/runtimePolicySession";
import {
	STATIC_BOOTSTRAP_ORIGIN,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
} from "./primaryBackend";

export function useBackendDirectory(args: {
	adminToken: string;
	clusterId: string | undefined;
	primaryClusterId: string | null;
	primaryOrigin: string;
	runtimeOnline: boolean;
	staticConsole: boolean;
	runtimePolicyReady: boolean;
	selectorOpen: boolean;
	refreshRuntimePolicy: () => Promise<{
		data?: RuntimePolicy;
		isSuccess: boolean;
	}>;
}) {
	const enabled =
		args.adminToken.length > 0 &&
		Boolean(args.clusterId) &&
		args.clusterId === args.primaryClusterId &&
		args.runtimeOnline &&
		(!args.staticConsole || args.runtimePolicyReady);
	const queryKeySuffix = [
		args.adminToken,
		args.primaryOrigin,
		args.clusterId,
	] as const;
	const backendNodes = useQuery({
		queryKey: ["adminNodes", ...queryKeySuffix],
		enabled,
		queryFn: ({ signal }) => fetchAdminNodes(args.adminToken, signal),
	});
	const nodeVersions = useQuery({
		queryKey: ["adminNodeVersions", ...queryKeySuffix],
		enabled: enabled && args.selectorOpen,
		queryFn: ({ signal }) => fetchAdminNodeVersions(args.adminToken, signal),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
		refetchOnMount: false,
		refetchOnReconnect: false,
		refetchOnWindowFocus: false,
	});
	const backendNodeInventoryKey = backendNodes.data?.items
		.map((node) => node.node_id)
		.join(",");
	const nodeVersionsUpdatedAt = nodeVersions.dataUpdatedAt;
	useEffect(() => {
		const clusterId = args.clusterId;
		if (!clusterId) return;
		hydratePrimaryBackendProfile(
			clusterId,
			backendNodes.isSuccess ? backendNodes.data.items : undefined,
		);
	}, [args.clusterId, backendNodes.data?.items, backendNodes.isSuccess]);
	useEffect(() => {
		if (
			!nodeVersions.isSuccess ||
			!backendNodes.isSuccess ||
			!backendNodeInventoryKey ||
			!nodeVersionsUpdatedAt
		)
			return;
		hydrateBackendVersions(nodeVersions.data.items);
	}, [
		backendNodeInventoryKey,
		backendNodes.isSuccess,
		nodeVersions.data?.items,
		nodeVersionsUpdatedAt,
		nodeVersions.isSuccess,
	]);
	const revalidateBackendCandidate = useCallback(async () => {
		if (!args.adminToken || !args.primaryClusterId || !args.clusterId) {
			return false;
		}
		if (args.staticConsole) {
			const policyResult = await args.refreshRuntimePolicy();
			const policy = policyResult.data;
			if (
				!policyResult.isSuccess ||
				!policy ||
				policy.cluster_id !== args.clusterId ||
				Date.parse(policy.expires_at) <= Date.now()
			) {
				return false;
			}
			const apiOrigins = [STATIC_BOOTSTRAP_ORIGIN, ...policy.api_origins];
			hydrateStaticRuntimePolicy(args.clusterId, apiOrigins, {
				candidateOrigins: [
					...apiOrigins,
					...policy.node_states.map((node) => node.origin),
				],
				nodeStates: policy.node_states,
				policyId: policy.policy_id,
				policyGrant: {
					policy_id: policy.policy_id,
					cluster_id: policy.cluster_id,
					expires_at: policy.expires_at,
					api_origins: [...new Set(apiOrigins)],
				},
				activate: readAppliedRuntimePolicyId() === policy.policy_id,
			});
		}
		const inventory = await backendNodes.refetch();
		if (!inventory.isSuccess || !inventory.data) return false;
		hydratePrimaryBackendProfile(args.clusterId, inventory.data.items);
		return true;
	}, [
		args.adminToken,
		args.clusterId,
		args.primaryClusterId,
		args.refreshRuntimePolicy,
		args.staticConsole,
		backendNodes.refetch,
	]);
	const refreshBackendCandidates = useCallback(() => {
		if (!args.adminToken || !args.primaryClusterId) return;
		if (args.staticConsole && !args.runtimePolicyReady) {
			void args.refreshRuntimePolicy();
			return;
		}
		if (args.staticConsole) void args.refreshRuntimePolicy();
		hydrateBackendVersions([]);
		if (backendNodes.isFetched) void backendNodes.refetch();
		if (nodeVersions.isFetched) void nodeVersions.refetch();
	}, [
		args.adminToken,
		args.primaryClusterId,
		args.refreshRuntimePolicy,
		backendNodes.isFetched,
		nodeVersions.isFetched,
		args.runtimePolicyReady,
		args.staticConsole,
		backendNodes.refetch,
		nodeVersions.refetch,
	]);

	return { nodeVersions, refreshBackendCandidates, revalidateBackendCandidate };
}
