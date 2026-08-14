import { afterEach, describe, expect, it, vi } from "vitest";

import { replaceAdminHistoryRepositories } from "./adminHistoryRepositories";
import { fetchAdminRepositoryHistory } from "./adminRepositoryHistory";

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("fetchAdminRepositoryHistory", () => {
	it("keeps the node scope and page cursor in the bounded query", async () => {
		const fetchMock = vi.fn().mockResolvedValue(
			new Response(
				JSON.stringify({
					repository: "repository-a",
					completeness: "complete",
					coverage: null,
					watermarks: [],
					gaps: [],
					clock_skew_seconds: 0,
					records: [],
					records_truncated: false,
				}),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			),
		);
		vi.stubGlobal("fetch", fetchMock);

		await fetchAdminRepositoryHistory("admin-token", {
			startUnixSeconds: 100,
			endUnixSeconds: 200,
			pageSize: 100,
			pageCursor: "25",
			subjectNodeId: "node-a",
		});
		const expectedUrl = [
			"/api/admin/history-repository?start_unix_seconds=100&end_unix_seconds=200",
			"&page_size=100&page_cursor=25&subject_node_id=node-a",
		].join("");

		expect(fetchMock).toHaveBeenCalledWith(
			expectedUrl,
			expect.objectContaining({
				method: "GET",
				headers: expect.objectContaining({
					Authorization: "Bearer admin-token",
				}),
			}),
		);

		fetchMock.mockResolvedValueOnce(
			new Response(JSON.stringify({ members: [] }), { status: 200 }),
		);
		await replaceAdminHistoryRepositories("admin-token", ["node-a"]);
		expect(fetchMock).toHaveBeenLastCalledWith(
			"/api/admin/history-repositories",
			expect.objectContaining({
				method: "PUT",
				body: JSON.stringify({ node_ids: ["node-a"] }),
			}),
		);
	});
});
