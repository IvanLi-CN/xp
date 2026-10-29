import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";

import {
	clearAuthenticatedBackendReadiness,
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	probeBackendOrigin,
	setAuthenticatedBackendReadiness,
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
		clusterMismatch: false,
		revalidating: false,
	});
	const previousOnline = useRef(isOnline);
	const probeStateRef = useRef(probeState);
	const returningOnlineRef = useRef(false);
	const returningOnline = isOnline && !previousOnline.current;
	probeStateRef.current = probeState;
	returningOnlineRef.current = returningOnline;
	const requestReady =
		staticConsole ||
		!isOnline ||
		(!returningOnline &&
			probeState.origin === origin &&
			probeState.clusterId === clusterId &&
			probeState.ready);
	useLayoutEffect(() => {
		setAuthenticatedBackendReadiness({
			origin,
			clusterId,
			ready: staticConsole || (isOnline && requestReady),
		});
		return clearAuthenticatedBackendReadiness;
	}, [origin, clusterId, staticConsole, isOnline, requestReady]);
	const probe = useCallback(
		async (signal?: AbortSignal): Promise<boolean> => {
			if (staticConsole || !isOnline) {
				setAuthenticatedBackendReadiness({
					origin,
					clusterId,
					ready: staticConsole,
				});
				setProbeState({
					clusterId,
					origin,
					ready: true,
					clusterMismatch: false,
					revalidating: false,
				});
				return true;
			}
			const revalidating =
				!returningOnlineRef.current &&
				probeStateRef.current.origin === origin &&
				probeStateRef.current.clusterId === clusterId &&
				probeStateRef.current.ready;
			setAuthenticatedBackendReadiness({ origin, clusterId, ready: false });
			setProbeState((current) => ({
				...current,
				clusterId,
				origin,
				ready: false,
				clusterMismatch: false,
				revalidating,
			}));
			const result = await probeBackendOrigin({ origin, signal });
			if (signal?.aborted) return false;
			const current = getPrimaryBackendSnapshot();
			if (current.primaryOrigin !== origin || current.clusterId !== clusterId) {
				return false;
			}
			if (clusterId === null || result.clusterId !== clusterId) {
				hydratePrimaryBackendProfile(result.clusterId);
				setProbeState({
					clusterId,
					origin,
					ready: false,
					clusterMismatch: true,
					revalidating: false,
				});
				return false;
			}
			setAuthenticatedBackendReadiness({ origin, clusterId, ready: true });
			setProbeState({
				clusterId,
				origin,
				ready: true,
				clusterMismatch: false,
				revalidating: false,
			});
			return true;
		},
		[clusterId, isOnline, origin, staticConsole],
	);
	const ready = requestReady;
	useEffect(() => {
		previousOnline.current = isOnline;
	}, [isOnline]);
	useEffect(() => {
		if (staticConsole || !isOnline) return;
		const controller = new AbortController();
		void probe(controller.signal).catch(() => undefined);
		return () => controller.abort();
	}, [isOnline, probe, staticConsole]);
	return {
		probe,
		ready,
		clusterMismatch: probeState.clusterMismatch,
		revalidating: probeState.revalidating,
	};
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
