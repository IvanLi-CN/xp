import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";

export const AdminTcpConnectionUsageWindowSchema = z.enum(["24h", "7d"]);

export const AdminTcpConnectionUsageWarningSchema = z.object({
	code: z.string(),
	message: z.string(),
});

export const AdminTcpConnectionEndpointOptionSchema = z.object({
	endpoint_id: z.string(),
	endpoint_tag: z.string(),
	port: z.number().int().nonnegative(),
});

export const AdminTcpConnectionSeriesPointSchema = z.object({
	minute: z.string(),
	count: z.number().int().nonnegative(),
});

export const AdminTcpConnectionEndpointSeriesSchema = z.object({
	endpoint_id: z.string(),
	endpoint_tag: z.string(),
	port: z.number().int().nonnegative(),
	series: z.array(AdminTcpConnectionSeriesPointSchema),
});

export const AdminNodeTcpConnectionsResponseSchema = z.object({
	node: AdminNodeSchema,
	window: AdminTcpConnectionUsageWindowSchema,
	window_start: z.string(),
	window_end: z.string(),
	warnings: z.array(AdminTcpConnectionUsageWarningSchema),
	endpoints: z.array(AdminTcpConnectionEndpointOptionSchema),
	per_endpoint_series: z.array(AdminTcpConnectionEndpointSeriesSchema),
});

export type AdminTcpConnectionUsageWindow = z.infer<
	typeof AdminTcpConnectionUsageWindowSchema
>;
export type AdminTcpConnectionUsageWarning = z.infer<
	typeof AdminTcpConnectionUsageWarningSchema
>;
export type AdminTcpConnectionEndpointOption = z.infer<
	typeof AdminTcpConnectionEndpointOptionSchema
>;
export type AdminTcpConnectionSeriesPoint = z.infer<
	typeof AdminTcpConnectionSeriesPointSchema
>;
export type AdminTcpConnectionEndpointSeries = z.infer<
	typeof AdminTcpConnectionEndpointSeriesSchema
>;
export type AdminNodeTcpConnectionsResponse = z.infer<
	typeof AdminNodeTcpConnectionsResponseSchema
>;

export async function fetchAdminNodeTcpConnections(
	adminToken: string,
	nodeId: string,
	window: AdminTcpConnectionUsageWindow,
	signal?: AbortSignal,
): Promise<AdminNodeTcpConnectionsResponse> {
	const res = await fetch(
		`/api/admin/nodes/${encodeURIComponent(nodeId)}/tcp-connections?window=${window}`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
	);

	await throwIfNotOk(res);
	const json: unknown = await res.json();
	return AdminNodeTcpConnectionsResponseSchema.parse(json);
}
