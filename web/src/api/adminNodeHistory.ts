import { z } from "zod";

import {
	NodeRuntimeEventSchema,
	RuntimeComponentSchema,
	RuntimeStatusSchema,
} from "./adminNodeRuntime";
import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";

export const NodeHistoryDailyTrafficSchema = z.object({
	date: z.string(),
	uplink_bytes: z.number().int().nonnegative(),
	downlink_bytes: z.number().int().nonnegative(),
	updated_at: z.string(),
});

export const NodeHistoryComponentDayStatusSchema = z.object({
	component: RuntimeComponentSchema,
	status: RuntimeStatusSchema,
	observed_at: z.string(),
});

export const NodeHistoryDailyComponentStatusSchema = z.object({
	date: z.string(),
	components: z.array(NodeHistoryComponentDayStatusSchema),
});

export const NodeHistoryComponentStatusEventSchema =
	NodeRuntimeEventSchema.pick({
		event_id: true,
		occurred_at: true,
		component: true,
		message: true,
		from_status: true,
		to_status: true,
	});

export const NodeHistorySnapshotSchema = z.object({
	node_id: z.string(),
	last_synced_at: z.string().nullable().optional(),
	last_sync_error: z.string().nullable().optional(),
	daily_traffic: z.array(NodeHistoryDailyTrafficSchema),
	daily_component_status: z.array(NodeHistoryDailyComponentStatusSchema),
	component_status_events: z.array(NodeHistoryComponentStatusEventSchema),
});

export const AdminNodeHistoryResponseSchema = z.object({
	node: AdminNodeSchema,
	history: NodeHistorySnapshotSchema.nullable().optional(),
});

export type NodeHistorySnapshot = z.infer<typeof NodeHistorySnapshotSchema>;
export type AdminNodeHistoryResponse = z.infer<
	typeof AdminNodeHistoryResponseSchema
>;

export async function fetchAdminNodeHistory(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<AdminNodeHistoryResponse> {
	const res = await fetch(
		`/api/admin/nodes/${encodeURIComponent(nodeId)}/history`,
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
	return AdminNodeHistoryResponseSchema.parse(json);
}
