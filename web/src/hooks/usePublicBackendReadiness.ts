import { useCallback, useEffect } from "react";

import {
	hydratePrimaryBackendProfile,
	probeBackendOrigin,
} from "../backend/primaryBackend";

type BackendNode = {
	node_id: string;
	node_name: string;
	api_base_url: string;
};

export function useAuthenticatedBackendProbe(args: {
	staticConsole: boolean;
	origin: string;
	clusterId: string | null;
}) {
	const { clusterId, origin, staticConsole } = args;
	return useCallback(
		async (signal?: AbortSignal) => {
			if (staticConsole) return;
			const result = await probeBackendOrigin({ origin, signal });
			if (result.clusterId !== clusterId) {
				hydratePrimaryBackendProfile(result.clusterId);
			}
		},
		[clusterId, origin, staticConsole],
	);
}

export function useHydratePrimaryBackendProfile(args: {
	clusterId?: string;
	primaryClusterId: string | null;
	nodes?: BackendNode[];
	nodesReady: boolean;
}) {
	const { clusterId, nodes, nodesReady, primaryClusterId } = args;
	useEffect(() => {
		if (!clusterId) return;
		const inventory =
			clusterId === primaryClusterId && nodesReady ? nodes : undefined;
		hydratePrimaryBackendProfile(clusterId, inventory);
	}, [clusterId, nodes, nodesReady, primaryClusterId]);
}
