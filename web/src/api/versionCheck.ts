import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const VersionCheckResponseSchema = z.object({
	current: z.object({
		package: z.string(),
		release_tag: z.string(),
	}),
	latest: z.object({
		release_tag: z.string(),
		published_at: z.string().optional(),
	}),
	has_update: z.boolean().nullable(),
	checked_at: z.string(),
	compare_reason: z.string(),
	source: z.object({
		kind: z.string(),
		repo: z.string(),
		api_base: z.string(),
		channel: z.string(),
	}),
});

export type VersionCheckResponse = z.infer<typeof VersionCheckResponseSchema>;

export async function fetchVersionCheck(options?: {
	signal?: AbortSignal;
	refresh?: boolean;
	adminToken?: string;
}): Promise<VersionCheckResponse> {
	const url = options?.refresh
		? "/api/version/check?refresh=1"
		: "/api/version/check";
	const res = await fetch(url, {
		method: "GET",
		headers: {
			Accept: "application/json",
			...(options?.refresh && options.adminToken
				? { Authorization: `Bearer ${options.adminToken}` }
				: {}),
		},
		signal: options?.signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return VersionCheckResponseSchema.parse(json);
}
