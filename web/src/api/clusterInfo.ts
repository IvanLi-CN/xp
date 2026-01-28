import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const ClusterInfoResponseSchema = z.object({
	cluster_id: z.string(),
	node_id: z.string(),
	role: z.string(),
	leader_api_base_url: z.string(),
	term: z.number().int().nonnegative(),
	xp_version: z.string(),
});

export type ClusterInfoResponse = z.infer<typeof ClusterInfoResponseSchema>;

export async function fetchClusterInfo(
	signal?: AbortSignal,
): Promise<ClusterInfoResponse> {
	const res = await fetch("/api/cluster/info", {
		method: "GET",
		headers: { Accept: "application/json" },
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return ClusterInfoResponseSchema.parse(json);
}
