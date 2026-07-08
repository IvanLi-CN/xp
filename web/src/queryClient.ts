import { QueryClient } from "@tanstack/react-query";

export function createQueryClient() {
	return new QueryClient({
		defaultOptions: {
			queries: {
				gcTime: 24 * 60 * 60 * 1000,
				retry: false,
				staleTime: 5_000,
			},
		},
	});
}
