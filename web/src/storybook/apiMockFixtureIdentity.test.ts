import { describe, expect, it } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { createMockApi } from "../../.storybook/mocks/apiMock";

const baseUrl = "http://localhost";

function jsonRequest(path: string, init?: RequestInit) {
	return new Request(`${baseUrl}${path}`, init);
}

describe("storybook API mock fixture identities", () => {
	it("starts synthetic identifier factories independently for each mock", async () => {
		const firstMock = createMockApi();
		const secondMock = createMockApi();
		const createUser = (mock: ReturnType<typeof createMockApi>) =>
			mock.handle(
				jsonRequest("/api/admin/users", {
					method: "POST",
					headers: { "Content-Type": "application/json" },
					body: JSON.stringify({ display_name: "Fixture user" }),
				}),
			);

		const [firstResponse, secondResponse] = await Promise.all([
			createUser(firstMock),
			createUser(secondMock),
		]);
		expect(firstResponse.ok).toBe(true);
		expect(secondResponse.ok).toBe(true);

		const first = (await firstResponse.json()) as {
			user_id: string;
			subscription_token: string;
		};
		const second = (await secondResponse.json()) as typeof first;
		expect(second).toEqual(first);
	});

	it("allocates distinct synthetic IDs for consecutive endpoint creates", async () => {
		const mock = createMockApi();
		const createEndpoint = (kind: "vless" | "ss") =>
			mock.handle(
				jsonRequest("/api/admin/endpoints", {
					method: "POST",
					headers: { "Content-Type": "application/json" },
					body: JSON.stringify({
						node_id: fixtureCatalog.nodeId.fixture32(),
						kind:
							kind === "ss"
								? fixtureCatalog.endpoint.ssKind()
								: fixtureCatalog.endpoint.vlessKind(),
						port:
							kind === "ss"
								? fixtureCatalog.endpoint.port8443()
								: fixtureCatalog.endpoint.port9443(),
					}),
				}),
			);

		const [firstResponse, secondResponse] = await Promise.all([
			createEndpoint("vless"),
			createEndpoint("ss"),
		]);
		expect(firstResponse.ok).toBe(true);
		expect(secondResponse.ok).toBe(true);
		const first = (await firstResponse.json()) as { endpoint_id: string };
		const second = (await secondResponse.json()) as typeof first;
		expect(first.endpoint_id).not.toBe(second.endpoint_id);

		for (const endpointId of [first.endpoint_id, second.endpoint_id]) {
			const response = await mock.handle(
				jsonRequest(`/api/admin/endpoints/${endpointId}`, { method: "GET" }),
			);
			expect(response.ok).toBe(true);
		}
	});
});
