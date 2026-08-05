import { useQuery } from "@tanstack/react-query";

import { fetchApiCompatibility } from "./apiCompatibility";

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
