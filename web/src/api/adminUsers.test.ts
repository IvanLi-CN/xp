import { afterEach, describe, expect, it, vi } from "vitest";

import { previewAdminUserMihomoImport } from "./adminUsers";

describe("previewAdminUserMihomoImport", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it("posts the fixed preview payload and parses the report", async () => {
		const fetchMock = vi.fn().mockResolvedValue(
			new Response(
				JSON.stringify({
					detected_source: "raw",
					added_proxies_yaml: "- name: imported\n  type: ss\n",
					counts: { added: 1, skipped: 0, failed: 0, warnings: 0 },
					items: [
						{
							source_index: 1,
							status: "added",
							name: "imported",
							scheme: "ss",
							warnings: [],
						},
					],
				}),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			),
		);
		vi.stubGlobal("fetch", fetchMock);

		await expect(
			previewAdminUserMihomoImport("admin-token", "user-1", {
				source: "ss://example",
				existing_extra_proxies_yaml: "",
			}),
		).resolves.toMatchObject({
			detected_source: "raw",
			counts: { added: 1 },
		});

		expect(fetchMock).toHaveBeenCalledWith(
			"/api/admin/users/user-1/subscription-mihomo-import/preview",
			expect.objectContaining({
				method: "POST",
				headers: expect.objectContaining({
					Authorization: "Bearer admin-token",
					"Cache-Control": "no-store",
				}),
				body: JSON.stringify({
					source: "ss://example",
					existing_extra_proxies_yaml: "",
				}),
			}),
		);
	});
});
