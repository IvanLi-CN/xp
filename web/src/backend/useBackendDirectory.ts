import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";

import { fetchAdminNodeVersions } from "@/api/adminNodeVersions";
import {
	type AdminNodesResponse,
	AdminNodesResponseSchema,
	fetchAdminNodes,
} from "@/api/adminNodes";
import type { RuntimePolicy } from "@/api/runtimePolicy";
import { RuntimePolicySchema } from "@/api/runtimePolicy";
import { readAppliedRuntimePolicyId } from "@/offline/runtimePolicySession";
import {
	STATIC_BOOTSTRAP_ORIGIN,
	applyDirectoryResponse,
	beginDirectoryRequest,
	canUseSupersededRevalidation,
	canonicalBackendOrigin,
	getDirectorySessionEpoch,
	getPrimaryBackendSnapshot,
	getStaticRuntimePolicyOrigins,
	hasDirectoryInventory,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
	readSwitchCandidateResource,
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
	const failedDirectoryRequest = useRef({ epoch: sessionEpoch, serial: 0 });
	const versionRefreshGeneration = useRef(0);
	const recordDirectoryFailure = useCallback(
		(serial: number) => {
			if (getDirectorySessionEpoch() !== sessionEpoch) return;
			if (
				failedDirectoryRequest.current.epoch !== sessionEpoch ||
				serial > failedDirectoryRequest.current.serial
			) {
				failedDirectoryRequest.current = { epoch: sessionEpoch, serial };
			}
		},
		[sessionEpoch],
	);
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
			try {
				const inventory = await fetchAdminNodes(args.adminToken, signal);
				return { inventory, serial };
			} catch (error) {
				recordDirectoryFailure(serial);
				throw error;
			}
		},
	});
	const nodeVersions = useQuery({
		queryKey: ["adminNodeVersions", sessionEpoch, ...queryKeySuffix],
		enabled: enabled && args.selectorOpen,
		queryFn: async ({ signal }) => {
			const generation = versionRefreshGeneration.current;
			return {
				response: await fetchAdminNodeVersions(args.adminToken, signal),
				generation,
			};
		},
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
			!args.selectorOpen ||
			!hasInventory ||
			!nodeVersions.isSuccess ||
			nodeVersions.isFetching ||
			nodeVersions.data.generation !== versionRefreshGeneration.current ||
			!backendNodes.isSuccess ||
			!backendNodeInventoryKey ||
			!nodeVersionsUpdatedAt
		)
			return;
		hydrateBackendVersions(nodeVersions.data.response.items);
	}, [
		backendNodeInventoryKey,
		args.selectorOpen,
		enabled,
		hasInventory,
		backendNodes.isSuccess,
		nodeVersions.data,
		nodeVersionsUpdatedAt,
		nodeVersions.isSuccess,
		nodeVersions.isFetching,
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
					policy = RuntimePolicySchema.parse(
						await readSwitchCandidateResource({
							origin: candidate.origin,
							clusterId: args.clusterId,
							adminToken: args.adminToken,
							path: "/api/admin/console/runtime-policy",
						}),
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
				const origin = canonicalBackendOrigin(candidate.origin);
				if (
					!origin ||
					!apiOrigins.some(
						(value) => canonicalBackendOrigin(value) === origin,
					) ||
					!getStaticRuntimePolicyOrigins().includes(origin) ||
					policy.node_states.some(
						(node) =>
							canonicalBackendOrigin(node.origin) === origin &&
							node.state === "temporarily_unavailable",
					)
				)
					return false;
			}
			let inventory: AdminNodesResponse;
			const serial = beginDirectoryRequest();
			try {
				inventory = AdminNodesResponseSchema.parse(
					await readSwitchCandidateResource({
						origin: candidate.origin,
						clusterId: args.clusterId,
						adminToken: args.adminToken,
						path: "/api/admin/nodes",
					}),
				);
			} catch {
				recordDirectoryFailure(serial);
				return false;
			}
			if (!isCurrent()) return false;
			const applied = applyDirectoryResponse(
				args.clusterId,
				sessionEpoch,
				serial,
				inventory.items,
			);
			if (!applied) {
				const failed = failedDirectoryRequest.current;
				const stillRegistered = getPrimaryBackendSnapshot().candidates.some(
					(item) =>
						item.nodeId === candidate.nodeId &&
						canonicalBackendOrigin(item.origin) ===
							canonicalBackendOrigin(candidate.origin),
				);
				if (
					failed.epoch !== sessionEpoch ||
					!stillRegistered ||
					!canUseSupersededRevalidation(serial, failed.serial)
				)
					return false;
			}
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
			recordDirectoryFailure,
			sessionEpoch,
		],
	);
	const refreshBackendCandidates = useCallback(() => {
		if (!args.adminToken || !args.primaryClusterId) return;
		versionRefreshGeneration.current++;
		hydrateBackendVersions([]);
		if (args.staticConsole && !args.runtimePolicyReady) {
			void args.refreshRuntimePolicy();
			return;
		}
		if (args.staticConsole) void args.refreshRuntimePolicy();
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
