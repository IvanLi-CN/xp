import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useState } from "react";

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

	useEffect(() => {
		const unsubscribe = subscribePrimaryBackend(setSnapshot);
		return unsubscribe;
	}, []);

	return (
		<PrimaryBackendContext.Provider value={snapshotOverride ?? snapshot}>
			{children}
		</PrimaryBackendContext.Provider>
	);
}

export function usePrimaryBackend() {
	return useContext(PrimaryBackendContext);
}
