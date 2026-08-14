import { z } from "zod";

import { throwIfNotOk } from "./backendError";

const RepositoryHistoryRangeSchema = z.object({
	start_unix_seconds: z.number(),
	end_unix_seconds: z.number(),
});

export const AdminRepositoryHistorySchema = z.object({
	repository: z.string().nullable(),
	completeness: z.enum(["complete", "partial", "local_only"]),
	coverage: z
		.object({
			observed: RepositoryHistoryRangeSchema,
			received: RepositoryHistoryRangeSchema,
		})
		.nullable(),
	watermarks: z.array(
		z.object({
			source_node_id: z.string(),
			source_epoch: z.number(),
			stream: z.string(),
			sequence: z.number(),
		}),
	),
	gaps: z.array(
		z.object({
			range: RepositoryHistoryRangeSchema,
			permanent: z.boolean(),
		}),
	),
	clock_skew_seconds: z.number(),
	records: z.array(z.unknown()),
	records_truncated: z.boolean(),
	next_page_cursor: z.string().optional(),
});

export type AdminRepositoryHistory = z.infer<
	typeof AdminRepositoryHistorySchema
>;

export async function fetchAdminRepositoryHistory(
	adminToken: string,
	query: {
		startUnixSeconds: number;
		endUnixSeconds: number;
		pageSize: number;
		pageCursor?: string;
		subjectNodeId?: string;
	},
	signal?: AbortSignal,
): Promise<AdminRepositoryHistory> {
	const params = new URLSearchParams({
		start_unix_seconds: String(query.startUnixSeconds),
		end_unix_seconds: String(query.endUnixSeconds),
		page_size: String(query.pageSize),
	});
	if (query.pageCursor) params.set("page_cursor", query.pageCursor);
	if (query.subjectNodeId) params.set("subject_node_id", query.subjectNodeId);
	const response = await fetch(`/api/admin/history-repository?${params}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});
	await throwIfNotOk(response);
	return AdminRepositoryHistorySchema.parse(await response.json());
}
