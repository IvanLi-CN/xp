import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";

import { fetchAdminNodeVersions } from "@/api/adminNodeVersions";
import { type AdminNodesResponse, fetchAdminNodes } from "@/api/adminNodes";
import type { RuntimePolicy } from "@/api/runtimePolicy";
import { fetchRuntimePolicy } from "@/api/runtimePolicy";
import { readAppliedRuntimePolicyId } from "@/offline/runtimePolicySession";
import {
	STATIC_BOOTSTRAP_ORIGIN,
	applyDirectoryResponse,
	beginDirectoryRequest,
	canonicalBackendOrigin,
	getDirectorySessionEpoch,
	hasDirectoryInventory,
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
	const currentArgs = useRef(args);
	currentArgs.current = args;
	const sessionEpoch = getDirectorySessionEpoch();
	const identity = `${sessionEpoch}:${args.clusterId ?? ""}:${args.adminToken}`;
	const [loadedIdentity, setLoadedIdentity] = useState("");
	const hasInventory = Boolean(
		args.clusterId &&
			args.adminToken &&
			(loadedIdentity === identity ||
				hasDirectoryInventory(args.clusterId, sessionEpoch)),
	);
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
		queryKey: ["backendDirectory", sessionEpoch, ...queryKeySuffix],
		enabled,
		queryFn: async ({ signal }) => {
			const serial = beginDirectoryRequest();
			const inventory = await fetchAdminNodes(args.adminToken, signal);
			return { inventory, serial };
		},
	});
	const nodeVersions = useQuery({
		queryKey: ["adminNodeVersions", sessionEpoch, ...queryKeySuffix],
		enabled: enabled && args.selectorOpen,
		queryFn: ({ signal }) => fetchAdminNodeVersions(args.adminToken, signal),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
		refetchOnMount: false,
		refetchOnReconnect: false,
		refetchOnWindowFocus: false,
	});
	const backendNodeInventoryKey = backendNodes.data?.inventory.items
		.map((node) => node.node_id)
		.join(",");
	const nodeVersionsUpdatedAt = nodeVersions.dataUpdatedAt;
	useEffect(() => {
		if (
			args.adminToken &&
			args.clusterId &&
			args.clusterId !== args.primaryClusterId
		) {
			hydratePrimaryBackendProfile(args.clusterId);
		}
	}, [args.adminToken, args.clusterId, args.primaryClusterId]);
	useEffect(() => {
		const clusterId = args.clusterId;
		if (
			!clusterId ||
			!enabled ||
			!backendNodes.isSuccess ||
			!backendNodes.isFetchedAfterMount ||
			backendNodes.isFetching
		)
			return;
		if (
			applyDirectoryResponse(
				clusterId,
				sessionEpoch,
				backendNodes.data.serial,
				backendNodes.data.inventory.items,
			)
		) {
			setLoadedIdentity(identity);
		}
	}, [
		args.clusterId,
		backendNodes.data,
		backendNodes.isSuccess,
		backendNodes.isFetchedAfterMount,
		backendNodes.isFetching,
		enabled,
		identity,
		sessionEpoch,
	]);
	useEffect(() => {
		if (
			!enabled ||
			!hasInventory ||
			!nodeVersions.isSuccess ||
			!backendNodes.isSuccess ||
			!backendNodeInventoryKey ||
			!nodeVersionsUpdatedAt
		)
			return;
		hydrateBackendVersions(nodeVersions.data.items);
	}, [
		backendNodeInventoryKey,
		enabled,
		hasInventory,
		backendNodes.isSuccess,
		nodeVersions.data?.items,
		nodeVersionsUpdatedAt,
		nodeVersions.isSuccess,
	]);
	const revalidateBackendCandidate = useCallback(
		async (candidate: { origin: string; nodeId: string }) => {
			if (!args.adminToken || !args.primaryClusterId || !args.clusterId) {
				return false;
			}
			const isCurrent = () =>
				getDirectorySessionEpoch() === sessionEpoch &&
				currentArgs.current.adminToken === args.adminToken &&
				currentArgs.current.clusterId === args.clusterId &&
				currentArgs.current.primaryClusterId === args.primaryClusterId &&
				currentArgs.current.primaryOrigin === args.primaryOrigin;
			if (args.staticConsole) {
				let policy: RuntimePolicy;
				try {
					policy = await fetchRuntimePolicy(
						args.adminToken,
						undefined,
						candidate.origin,
					);
				} catch {
					return false;
				}
				if (
					!isCurrent() ||
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
			let inventory: AdminNodesResponse;
			const serial = beginDirectoryRequest();
			try {
				inventory = await fetchAdminNodes(
					args.adminToken,
					undefined,
					candidate.origin,
				);
			} catch {
				return false;
			}
			if (
				!isCurrent() ||
				!applyDirectoryResponse(
					args.clusterId,
					sessionEpoch,
					serial,
					inventory.items,
				)
			)
				return false;
			return inventory.items.some(
				(node) =>
					node.node_id === candidate.nodeId &&
					canonicalBackendOrigin(node.api_base_url) ===
						canonicalBackendOrigin(candidate.origin),
			);
		},
		[
			args.adminToken,
			args.clusterId,
			args.primaryClusterId,
			args.staticConsole,
			args.primaryOrigin,
			sessionEpoch,
		],
	);
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

	const unavailable =
		!args.runtimeOnline ||
		(args.staticConsole && !args.runtimePolicyReady) ||
		backendNodes.isError;
	const directoryStatus: "stale" | "error" | "ready" | "loading" = unavailable
		? hasInventory
			? "stale"
			: "error"
		: hasInventory
			? "ready"
			: "loading";
	return {
		nodeVersions,
		refreshBackendCandidates,
		revalidateBackendCandidate,
		directoryStatus,
	};
}
