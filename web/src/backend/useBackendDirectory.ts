import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect } from "react";

import { fetchAdminNodeVersions } from "@/api/adminNodeVersions";
import { fetchAdminNodes } from "@/api/adminNodes";
import {
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
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
	refreshRuntimePolicy: () => unknown;
}) {
	const enabled =
		args.adminToken.length > 0 &&
		Boolean(args.clusterId) &&
		args.clusterId === args.primaryClusterId &&
		args.runtimeOnline &&
		(!args.staticConsole || args.runtimePolicyReady);
	const queryKeySuffix = [args.adminToken, args.primaryOrigin] as const;
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
			!backendNodeInventoryKey
		)
			return;
		hydrateBackendVersions(nodeVersions.data.items);
	}, [
		backendNodeInventoryKey,
		backendNodes.isSuccess,
		nodeVersions.data?.items,
		nodeVersions.isSuccess,
	]);
	const refreshBackendCandidates = useCallback(() => {
		if (!args.adminToken || !args.primaryClusterId) return;
		if (args.staticConsole) void args.refreshRuntimePolicy();
		void backendNodes.refetch();
		void nodeVersions.refetch();
	}, [
		args.adminToken,
		args.primaryClusterId,
		args.refreshRuntimePolicy,
		args.staticConsole,
		backendNodes.refetch,
		nodeVersions.refetch,
	]);

	return { nodeVersions, refreshBackendCandidates };
}
