import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";
import { type SseMessage, type SseStreamHandle, startSseStream } from "./sse";

const RuntimeSummaryStatusSchema = z.enum([
	"up",
	"degraded",
	"down",
	"unknown",
]);
const RuntimeStatusSchema = z.enum(["disabled", "up", "down", "unknown"]);
const RuntimeComponentSchema = z.enum(["xp", "xray", "cloudflared"]);
const RuntimeEventKindSchema = z.enum([
	"status_changed",
	"restart_requested",
	"restart_succeeded",
	"restart_failed",
]);

export const NodeRuntimeSummarySchema = z.object({
	status: RuntimeSummaryStatusSchema,
	updated_at: z.string(),
});

export const NodeRuntimeComponentSchema = z.object({
	component: RuntimeComponentSchema,
	status: RuntimeStatusSchema,
	last_ok_at: z.string().nullable().optional(),
	last_fail_at: z.string().nullable().optional(),
	down_since: z.string().nullable().optional(),
	consecutive_failures: z.number(),
	recoveries_observed: z.number(),
	restart_attempts: z.number(),
	last_restart_at: z.string().nullable().optional(),
	last_restart_fail_at: z.string().nullable().optional(),
});

export const NodeRuntimeHistorySlotSchema = z.object({
	slot_start: z.string(),
	status: RuntimeSummaryStatusSchema,
});

export const NodeRuntimeEventSchema = z.object({
	event_id: z.string(),
	occurred_at: z.string(),
	component: RuntimeComponentSchema,
	kind: RuntimeEventKindSchema,
	message: z.string(),
	from_status: RuntimeStatusSchema.nullable().optional(),
	to_status: RuntimeStatusSchema.nullable().optional(),
});

export type NodeRuntimeSummary = z.infer<typeof NodeRuntimeSummarySchema>;
export type NodeRuntimeComponent = z.infer<typeof NodeRuntimeComponentSchema>;
export type NodeRuntimeHistorySlot = z.infer<
	typeof NodeRuntimeHistorySlotSchema
>;
export type NodeRuntimeEvent = z.infer<typeof NodeRuntimeEventSchema>;

export const AdminNodeRuntimeListItemSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	access_host: z.string(),
	summary: NodeRuntimeSummarySchema,
	components: z.array(NodeRuntimeComponentSchema),
	recent_slots: z.array(NodeRuntimeHistorySlotSchema),
});

export const AdminNodesRuntimeResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	items: z.array(AdminNodeRuntimeListItemSchema),
});

export type AdminNodeRuntimeListItem = z.infer<
	typeof AdminNodeRuntimeListItemSchema
>;
export type AdminNodesRuntimeResponse = z.infer<
	typeof AdminNodesRuntimeResponseSchema
>;

export const AdminNodeRuntimeDetailResponseSchema = z.object({
	node: AdminNodeSchema,
	summary: NodeRuntimeSummarySchema,
	components: z.array(NodeRuntimeComponentSchema),
	recent_slots: z.array(NodeRuntimeHistorySlotSchema),
	events: z.array(NodeRuntimeEventSchema),
});

export type AdminNodeRuntimeDetailResponse = z.infer<
	typeof AdminNodeRuntimeDetailResponseSchema
>;

const NodeRuntimeSseHelloSchema = z.object({
	node_id: z.string(),
	connected_at: z.string(),
});

const NodeRuntimeSseNodeErrorSchema = z.object({
	node_id: z.string(),
	error: z.string(),
});

const NodeRuntimeSseLaggedSchema = z.object({
	node_id: z.string(),
	missed: z.number(),
});

const NodeRuntimeSseSnapshotSchema = z.object({
	node_id: z.string(),
	summary: NodeRuntimeSummarySchema,
	components: z.array(NodeRuntimeComponentSchema),
	recent_slots: z.array(NodeRuntimeHistorySlotSchema),
	events: z.array(NodeRuntimeEventSchema),
});

export type NodeRuntimeSseParsedMessage =
	| { type: "hello"; data: z.infer<typeof NodeRuntimeSseHelloSchema> }
	| { type: "snapshot"; data: z.infer<typeof NodeRuntimeSseSnapshotSchema> }
	| { type: "event"; data: NodeRuntimeEvent }
	| { type: "node_error"; data: z.infer<typeof NodeRuntimeSseNodeErrorSchema> }
	| { type: "lagged"; data: z.infer<typeof NodeRuntimeSseLaggedSchema> };

export async function fetchAdminNodesRuntime(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminNodesRuntimeResponse> {
	const res = await fetch("/api/admin/nodes/runtime", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
	const json: unknown = await res.json();
	return AdminNodesRuntimeResponseSchema.parse(json);
}

export async function fetchAdminNodeRuntime(
	adminToken: string,
	nodeId: string,
	opts?: { eventsLimit?: number; signal?: AbortSignal },
): Promise<AdminNodeRuntimeDetailResponse> {
	const query =
		typeof opts?.eventsLimit === "number"
			? `?events_limit=${Math.max(0, Math.floor(opts.eventsLimit))}`
			: "";
	const res = await fetch(`/api/admin/nodes/${nodeId}/runtime${query}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal: opts?.signal,
	});

	await throwIfNotOk(res);
	const json: unknown = await res.json();
	return AdminNodeRuntimeDetailResponseSchema.parse(json);
}

function parseNodeRuntimeSse(
	msg: SseMessage,
): NodeRuntimeSseParsedMessage | null {
	if (!msg.event || !msg.data) return null;

	const parsedJson = JSON.parse(msg.data) as unknown;
	switch (msg.event) {
		case "hello":
			return {
				type: "hello",
				data: NodeRuntimeSseHelloSchema.parse(parsedJson),
			};
		case "snapshot":
			return {
				type: "snapshot",
				data: NodeRuntimeSseSnapshotSchema.parse(parsedJson),
			};
		case "event":
			return {
				type: "event",
				data: NodeRuntimeEventSchema.parse(parsedJson),
			};
		case "node_error":
			return {
				type: "node_error",
				data: NodeRuntimeSseNodeErrorSchema.parse(parsedJson),
			};
		case "lagged":
			return {
				type: "lagged",
				data: NodeRuntimeSseLaggedSchema.parse(parsedJson),
			};
		default:
			return null;
	}
}

export type StartNodeRuntimeEventsArgs = {
	adminToken: string;
	nodeId: string;
	onMessage: (msg: NodeRuntimeSseParsedMessage) => void;
	onOpen?: () => void;
	onError?: (error: unknown) => void;
	onClose?: () => void;
};

export function startNodeRuntimeEvents(
	args: StartNodeRuntimeEventsArgs,
): SseStreamHandle {
	return startSseStream({
		url: `/api/admin/nodes/${args.nodeId}/runtime/events`,
		headers: {
			Authorization: `Bearer ${args.adminToken}`,
		},
		onMessage: (raw) => {
			const parsed = parseNodeRuntimeSse(raw);
			if (parsed) args.onMessage(parsed);
		},
		onOpen: args.onOpen,
		onError: args.onError,
		onClose: args.onClose,
	});
}
