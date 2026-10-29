import { useCallback, useEffect, useState } from "react";

import {
	getPrimaryBackendSnapshot,
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
	isOnline: boolean;
	origin: string;
	clusterId: string | null;
}) {
	const { clusterId, isOnline, origin, staticConsole } = args;
	const [probeState, setProbeState] = useState({
		clusterId,
		origin,
		ready: staticConsole || !isOnline,
	});
	const probe = useCallback(
		async (signal?: AbortSignal): Promise<boolean> => {
			if (staticConsole || !isOnline) {
				setProbeState({ clusterId, origin, ready: true });
				return true;
			}
			setProbeState({ clusterId, origin, ready: false });
			const result = await probeBackendOrigin({ origin, signal });
			if (signal?.aborted) return false;
			const current = getPrimaryBackendSnapshot();
			if (current.primaryOrigin !== origin || current.clusterId !== clusterId) {
				return false;
			}
			if (result.clusterId !== clusterId) {
				hydratePrimaryBackendProfile(result.clusterId);
				return false;
			}
			setProbeState({ clusterId, origin, ready: true });
			return true;
		},
		[clusterId, isOnline, origin, staticConsole],
	);
	const ready =
		staticConsole ||
		!isOnline ||
		(probeState.origin === origin &&
			probeState.clusterId === clusterId &&
			probeState.ready);
	useEffect(() => {
		if (staticConsole || !isOnline) return;
		const controller = new AbortController();
		void probe(controller.signal).catch(() => undefined);
		return () => controller.abort();
	}, [isOnline, probe, staticConsole]);
	return { probe, ready };
}

export function useHydratePrimaryBackendProfile(args: {
	clusterId?: string;
	primaryClusterId: string | null;
	nodes?: BackendNode[];
	nodesReady: boolean;
	staticConsole?: boolean;
}) {
	const {
		clusterId,
		nodes,
		nodesReady,
		primaryClusterId,
		staticConsole = false,
	} = args;
	useEffect(() => {
		if (!clusterId || (!staticConsole && clusterId !== primaryClusterId))
			return;
		hydratePrimaryBackendProfile(clusterId, nodesReady ? nodes : undefined);
	}, [clusterId, nodes, nodesReady, primaryClusterId, staticConsole]);
}
