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

	useEffect(() => {
		const unsubscribe = subscribePrimaryBackend(setSnapshot);
		return unsubscribe;
	}, []);
	useEffect(() => {
		if (previousGeneration.current === snapshot.generation) return;
		previousGeneration.current = snapshot.generation;
		void queryClient.cancelQueries().then(async () => {
			queryClient.removeQueries();
			await clearPersistedQueryCache();
		});
	}, [queryClient, snapshot.generation]);

	return (
		<PrimaryBackendContext.Provider value={snapshotOverride ?? snapshot}>
			{children}
		</PrimaryBackendContext.Provider>
	);
}

export function usePrimaryBackend() {
	return useContext(PrimaryBackendContext);
}