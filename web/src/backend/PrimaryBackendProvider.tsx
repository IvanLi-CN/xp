import { useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useRef, useState } from "react";

import {
	clearBackendQueryCache,
	clearPersistedQueryCache,
} from "../offline/queryPersistence";
import {
	type PrimaryBackendSnapshot,
	getDirectorySessionEpoch,
	getPrimaryBackendSnapshot,
	subscribePrimaryBackend,
} from "./primaryBackend";

const PrimaryBackendContext = createContext<PrimaryBackendSnapshot>(
	getPrimaryBackendSnapshot(),
);

export function PrimaryBackendProvider({
	children,
	snapshot: snapshotOverride,
}: {
	children: ReactNode;
	snapshot?: PrimaryBackendSnapshot;
}) {
	const [snapshot, setSnapshot] = useState(getPrimaryBackendSnapshot);
	const queryClient = useQueryClient();
	const previousGeneration = useRef(snapshot.generation);
	const previousSnapshot = useRef(snapshot);
	const previousSessionEpoch = useRef(getDirectorySessionEpoch());

	useEffect(() => {
		const unsubscribe = subscribePrimaryBackend((next) => {
			const epoch = getDirectorySessionEpoch();
			if (epoch !== previousSessionEpoch.current) {
				previousSessionEpoch.current = epoch;
				clearBackendQueryCache(queryClient);
			}
			setSnapshot(next);
		});
		return unsubscribe;
	}, [queryClient]);
	useEffect(() => {
		const previous = previousSnapshot.current;
		previousSnapshot.current = snapshot;
		if (previousGeneration.current === snapshot.generation) return;
		previousGeneration.current = snapshot.generation;
		const identityChanged =
			previous.primaryOrigin !== snapshot.primaryOrigin ||
			(previous.clusterId !== null &&
				previous.clusterId !== snapshot.clusterId);
		if (!identityChanged) return;
		void queryClient.cancelQueries().then(async () => {
			queryClient.removeQueries();
			await clearPersistedQueryCache();
		});
	}, [queryClient, snapshot]);

	return (
		<PrimaryBackendContext.Provider value={snapshotOverride ?? snapshot}>
			{children}
		</PrimaryBackendContext.Provider>
	);
}

export function usePrimaryBackend() {
	return useContext(PrimaryBackendContext);
}
