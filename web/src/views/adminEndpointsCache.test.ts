import { describe, expect, it } from "vitest";

import type { AdminEndpointsResponse } from "../api/adminEndpoints";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { createQueryClient } from "../queryClient";
import {
	appendAdminEndpoint,
	removeAdminEndpoints,
	replaceAdminEndpoint,
	resourceListCache,
	syncNode,
} from "./adminEndpointsCache";

const endpoint = {
	endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
	node_id: fixtureCatalog.identifier.nodePrimary(),
	tag: fixtureCatalog.identifier.endpointTagPrimary(),
	kind: fixtureCatalog.endpoint.vlessKind(),
	port: fixtureCatalog.endpoint.port443(),
	meta: {},
};

const cached: AdminEndpointsResponse = {
	items: [
		endpoint,
		{
			...endpoint,
			endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
			tag: fixtureCatalog.identifier.endpointTagSecondary(),
		},
	],
};

describe("admin endpoint cache", () => {
	it("preserves API order while applying navigation list mutations", () => {
		expect(
			appendAdminEndpoint(cached, {
				...endpoint,
				endpoint_id: fixtureCatalog.identifier.endpointTertiary(),
			})?.items.map((item) => item.endpoint_id),
		).toEqual([
			fixtureCatalog.identifier.endpointPrimary(),
			fixtureCatalog.identifier.endpointSecondary(),
			fixtureCatalog.identifier.endpointTertiary(),
		]);
		expect(
			replaceAdminEndpoint(cached, {
				...endpoint,
				tag: fixtureCatalog.identifier.endpointTagTertiary(),
			})?.items[0].tag,
		).toBe(fixtureCatalog.identifier.endpointTagTertiary());
		expect(
			removeAdminEndpoints(cached, [
				fixtureCatalog.identifier.endpointPrimary(),
				fixtureCatalog.identifier.endpointSecondary(),
			])?.items,
		).toEqual([]);
	});

	it("synchronizes updates and removes deleted resource detail caches", () => {
		const queryClient = createQueryClient();
		queryClient.setQueryData(["adminNodes", "token"], {
			items: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					node_name: fixtureCatalog.identifier.nodeNamePrimary(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				},
			],
		});
		queryClient.setQueryData(
			["adminEndpoint", "token", fixtureCatalog.identifier.endpointPrimary()],
			endpoint,
		);
		queryClient.setQueryData(
			["adminNode", "token", fixtureCatalog.identifier.nodePrimary()],
			{ node_id: fixtureCatalog.identifier.nodePrimary() },
		);

		syncNode(queryClient, "token", fixtureCatalog.identifier.nodePrimary(), {
			quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		});
		expect(
			queryClient.getQueryData<{ items: Array<{ quota_limit_bytes: number }> }>(
				["adminNodes", "token"],
			)?.items[0].quota_limit_bytes,
		).toBe(fixtureCatalog.quota.limitBytes());

		resourceListCache.remove(
			queryClient,
			"token",
			fixtureCatalog.identifier.endpointPrimary(),
		);
		expect(
			queryClient.getQueryData([
				"adminEndpoint",
				"token",
				fixtureCatalog.identifier.endpointPrimary(),
			]),
		).toBeUndefined();
		queryClient.setQueryData(
			["adminEndpoint", "token", fixtureCatalog.identifier.endpointSecondary()],
			{
				...endpoint,
				endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
			},
		);
		resourceListCache.nodeDeleted(
			queryClient,
			"token",
			fixtureCatalog.identifier.nodePrimary(),
			[endpoint],
		);
		expect(
			queryClient.getQueryData([
				"adminNode",
				"token",
				fixtureCatalog.identifier.nodePrimary(),
			]),
		).toBeUndefined();
		expect(
			queryClient.getQueryData([
				"adminEndpoint",
				"token",
				fixtureCatalog.identifier.endpointSecondary(),
			]),
		).toEqual({
			...endpoint,
			endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
		});
	});
});
