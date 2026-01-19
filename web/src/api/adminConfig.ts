import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminConfigResponseSchema = z.object({
	bind: z.string(),
	xray_api_addr: z.string(),
	data_dir: z.string(),
	node_name: z.string(),
	access_host: z.string(),
	api_base_url: z.string(),
	quota_poll_interval_secs: z.number(),
	quota_auto_unban: z.boolean(),
	admin_token_present: z.boolean(),
	admin_token_masked: z.string(),
});

export type AdminConfigResponse = z.infer<typeof AdminConfigResponseSchema>;

export async function fetchAdminConfig(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminConfigResponse> {
	const res = await fetch("/api/admin/config", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminConfigResponseSchema.parse(json);
}
