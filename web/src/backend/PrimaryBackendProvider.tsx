import { useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useRef, useState } from "react";

import { clearPersistedQueryCache } from "../offline/queryPersistence";
import {
	type PrimaryBackendSnapshot,
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

	useEffect(() => {
		const unsubscribe = subscribePrimaryBackend(setSnapshot);
		return unsubscribe;
	}, []);
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
