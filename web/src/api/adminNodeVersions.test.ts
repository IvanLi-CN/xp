import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	AdminNodeVersionsResponseSchema,
	fetchAdminNodeVersions,
} from "./adminNodeVersions";

describe("admin node versions", () => {
	afterEach(() => vi.restoreAllMocks());

	it("parses bounded authenticated version results", async () => {
		const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(
				JSON.stringify({
					items: [
						{
							node_id: fixtureCatalog.identifier.nodePrimary(),
							xp_version: "v3.22.5",
							error: null,
						},
						{
							node_id: fixtureCatalog.identifier.nodeSecondary(),
							xp_version: null,
							error: "unreachable",
						},
					],
				}),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			),
		);

		await expect(fetchAdminNodeVersions("admin-token")).resolves.toEqual({
			items: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					xp_version: "v3.22.5",
					error: null,
				},
				{
					node_id: fixtureCatalog.identifier.nodeSecondary(),
					xp_version: null,
					error: "unreachable",
				},
			],
		});
		expect(fetchSpy).toHaveBeenCalledWith(
			"/api/admin/nodes/versions",
			expect.objectContaining({
				method: "GET",
				headers: expect.objectContaining({
					Authorization: "Bearer admin-token",
				}),
				signal: expect.any(AbortSignal),
			}),
		);
	});

	it("rejects malformed results instead of treating them as compatible", () => {
		expect(() =>
			AdminNodeVersionsResponseSchema.parse({
				items: [
					{
						node_id: fixtureCatalog.identifier.nodePrimary(),
						xp_version: "3.22.5",
					},
				],
			}),
		).toThrow();
	});
});
