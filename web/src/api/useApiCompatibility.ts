import { useQuery } from "@tanstack/react-query";
import {
	type ReactNode,
	createContext,
	createElement,
	useContext,
} from "react";

import {
	type ApiCompatibilityResult,
	fetchApiCompatibility,
} from "./apiCompatibility";
import type { ApiCapability } from "./releaseInventories";

const ApiCompatibilityContext = createContext<ApiCompatibilityResult | null>(
	null,
);

export function ApiCompatibilityProvider(props: {
	value: ApiCompatibilityResult | null;
	children: ReactNode;
}) {
	return createElement(
		ApiCompatibilityContext.Provider,
		{ value: props.value },
		props.children,
	);
}

export function useApiCompatibility(adminToken: string, isOnline: boolean) {
	return useQuery({
		queryKey: ["apiCompatibility", adminToken ? "admin" : "anonymous"],
		queryFn: ({ signal }) =>
			fetchApiCompatibility({
				adminToken: adminToken || undefined,
				signal,
			}),
		enabled: isOnline,
		staleTime: 5 * 60 * 1000,
		retry: false,
	});
}

export function useApiCapability(capability: ApiCapability) {
	const compatibility = useContext(ApiCompatibilityContext);
	const unavailable =
		compatibility?.kind === "incompatible" ||
		(compatibility?.kind === "compatible" &&
			!compatibility.isFeatureAvailable(capability));
	return {
		available: !unavailable,
		unavailable,
		reason:
			compatibility?.kind === "incompatible"
				? compatibility.reason
				: compatibility?.kind === "compatible"
					? compatibility.degradationFor(capability)
					: null,
	};
}
