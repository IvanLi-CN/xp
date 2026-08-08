import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const UpgradeJobStateSchema = z.enum([
	"idle",
	"running",
	"restarting",
	"succeeded",
	"failed",
	"unsupported",
]);

export const UpgradeSupportSchema = z.object({
	supported: z.boolean(),
	reason: z.string().nullable().optional(),
	trigger: z.string().nullable().optional(),
	storage: z
		.object({
			install: z.object({
				path: z.string(),
				available_bytes: z.number(),
				reclaimable_bytes: z.number(),
				required_bytes: z.number(),
				sufficient_after_cleanup: z.boolean(),
			}),
			workspace: z.object({
				path: z.string(),
				available_bytes: z.number(),
				reclaimable_bytes: z.number(),
				required_bytes: z.number(),
				sufficient_after_cleanup: z.boolean(),
			}),
			cleanup_required: z.boolean(),
		})
		.optional(),
});

export const UpgradeJobStatusSchema = z.object({
	state: UpgradeJobStateSchema,
	target_tag: z.string().nullable().optional(),
	repo: z.string().nullable().optional(),
	started_at: z.string().nullable().optional(),
	finished_at: z.string().nullable().optional(),
	exit_code: z.number().nullable().optional(),
	message: z.string().nullable().optional(),
	updated_at: z.string(),
});

export const AdminUpgradeStatusResponseSchema = z.object({
	support: UpgradeSupportSchema,
	status: UpgradeJobStatusSchema,
});

export type UpgradeJobState = z.infer<typeof UpgradeJobStateSchema>;
export type UpgradeSupport = z.infer<typeof UpgradeSupportSchema>;
export type UpgradeJobStatus = z.infer<typeof UpgradeJobStatusSchema>;
export type AdminUpgradeStatusResponse = z.infer<
	typeof AdminUpgradeStatusResponseSchema
>;

export async function fetchAdminUpgradeStatus(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminUpgradeStatusResponse> {
	const res = await fetch("/api/admin/upgrade/status", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUpgradeStatusResponseSchema.parse(json);
}

export async function startAdminUpgrade(
	adminToken: string,
	payload: { target_tag: string },
	signal?: AbortSignal,
): Promise<AdminUpgradeStatusResponse> {
	const res = await fetch("/api/admin/upgrade/start", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUpgradeStatusResponseSchema.parse(json);
}
