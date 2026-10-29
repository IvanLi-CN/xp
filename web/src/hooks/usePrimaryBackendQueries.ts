import { useQuery } from "@tanstack/react-query";
import { useCallback } from "react";

import { fetchClusterInfo } from "../api/clusterInfo";

export function usePrimaryClusterInfo() {
	return useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});
}

export function useRefreshBackendCandidates(args: {
	adminToken: string;
	clusterId: string | null;
	staticConsole: boolean;
	probe: () => Promise<boolean>;
	refetchInventory: () => Promise<unknown>;
	refetchPolicy: () => Promise<unknown>;
}) {
	const {
		adminToken,
		clusterId,
		probe,
		refetchInventory,
		refetchPolicy,
		staticConsole,
	} = args;
	return useCallback(() => {
		if (!adminToken || !clusterId) return;
		if (staticConsole) {
			void refetchPolicy();
			return;
		}
		void probe()
			.then((ready) => (ready ? refetchInventory() : undefined))
			.catch(() => undefined);
	}, [
		adminToken,
		clusterId,
		probe,
		refetchInventory,
		refetchPolicy,
		staticConsole,
	]);
}
