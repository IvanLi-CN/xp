import { z } from "zod";

import { AlertsResponseSchema } from "./adminAlerts";
import { AdminNodesRuntimeResponseSchema } from "./adminNodeRuntime";
import { AdminUpgradeStatusResponseSchema } from "./adminUpgrade";
import { ClusterInfoResponseSchema } from "./clusterInfo";
import { HealthResponseSchema } from "./health";
import { type SseStreamHandle, startSseStream } from "./sse";

const AdminStatusEventsHelloSchema = z.object({
	node_id: z.string(),
	connected_at: z.string(),
});

const AdminStatusEventsSnapshotSchema = z.object({
	emitted_at: z.string(),
	health: HealthResponseSchema,
	cluster_info: ClusterInfoResponseSchema,
	nodes_runtime: AdminNodesRuntimeResponseSchema,
	alerts: AlertsResponseSchema,
	upgrade: AdminUpgradeStatusResponseSchema,
});

const AdminStatusEventsErrorSchema = z.object({
	message: z.string(),
});

export type AdminStatusEventsMessage =
	| { type: "hello"; data: z.infer<typeof AdminStatusEventsHelloSchema> }
	| { type: "snapshot"; data: z.infer<typeof AdminStatusEventsSnapshotSchema> }
	| {
			type: "snapshot_error";
			data: z.infer<typeof AdminStatusEventsErrorSchema>;
	  };

function parseAdminStatusMessage(
	event: string,
	data: string,
): AdminStatusEventsMessage | null {
	let parsedJson: unknown;
	try {
		parsedJson = JSON.parse(data);
	} catch {
		return null;
	}

	if (event === "hello") {
		return {
			type: "hello",
			data: AdminStatusEventsHelloSchema.parse(parsedJson),
		};
	}
	if (event === "snapshot") {
		return {
			type: "snapshot",
			data: AdminStatusEventsSnapshotSchema.parse(parsedJson),
		};
	}
	if (event === "snapshot_error") {
		return {
			type: "snapshot_error",
			data: AdminStatusEventsErrorSchema.parse(parsedJson),
		};
	}
	return null;
}

export function startAdminStatusEvents(args: {
	adminToken: string;
	onMessage: (message: AdminStatusEventsMessage) => void;
	onOpen?: () => void;
	onClose?: () => void;
	onError?: (error: unknown) => void;
}): SseStreamHandle {
	return startSseStream({
		url: "/api/admin/status/events",
		headers: {
			Authorization: `Bearer ${args.adminToken}`,
		},
		onMessage: (message) => {
			const parsed = parseAdminStatusMessage(message.event, message.data);
			if (parsed) args.onMessage(parsed);
		},
		onOpen: args.onOpen,
		onClose: args.onClose,
		onError: args.onError,
	});
}
