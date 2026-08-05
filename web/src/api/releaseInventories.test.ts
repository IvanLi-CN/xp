import { execFileSync } from "node:child_process";

import { describe, expect, it } from "vitest";

import { AdminConfigResponseSchema } from "./adminConfig";
import { AdminNodesRuntimeResponseSchema } from "./adminNodeRuntime";
import { AdminNodesResponseSchema } from "./adminNodes";
import {
	AdminStatusEventsHelloSchema,
	AdminStatusEventsSnapshotSchema,
} from "./adminStatusEvents";
import { ClusterInfoResponseSchema } from "./clusterInfo";
import { HealthResponseSchema } from "./health";
import {
	API_CAPABILITIES_PATH,
	API_COMPATIBILITY_WINDOW,
	RELEASE_API_FIXTURES,
	RELEASE_COMPATIBILITY_CONTRACTS,
	RELEASE_INVENTORIES,
} from "./releaseInventories";
import { VersionCheckResponseSchema } from "./versionCheck";

function fixturePathMatches(template: string, actual: string): boolean {
	const templateParts = template.split("/");
	const actualParts = actual.split("/");
	return (
		templateParts.length === actualParts.length &&
		templateParts.every(
			(part, index) =>
				(part.startsWith("{") && part.endsWith("}")) ||
				part === actualParts[index],
		)
	);
}

function executeFixtureRequest(
	fixture: (typeof RELEASE_API_FIXTURES)[number],
	serverRoutes: readonly string[],
	route: string,
	request: Request,
): Response {
	const contract = fixture.requests.find((item) => item.route === route);
	const path = new URL(request.url).pathname;
	if (
		!contract ||
		request.method !== contract.method ||
		!fixturePathMatches(contract.path, path)
	) {
		return new Response(null, { status: 400 });
	}
	if (!serverRoutes.includes(route)) return new Response(null, { status: 404 });
	const response = fixture.responses.find((item) => item.route === route);
	if (contract.expectedStatus === 204) {
		return new Response(null, { status: 204 });
	}
	return new Response(
		response?.wireBody ??
			(response?.contentType === "text/plain"
				? String(response?.body)
				: JSON.stringify(response?.body ?? {})),
		{
			status: contract.expectedStatus,
			headers: {
				"Content-Type": response?.contentType ?? "application/json",
			},
		},
	);
}

describe("immutable release inventories", () => {
	it("pins the latest stable tag and source commit for every compatibility minor", () => {
		expect(RELEASE_INVENTORIES.map((inventory) => inventory.minor)).toEqual(
			API_COMPATIBILITY_WINDOW,
		);
		expect(
			RELEASE_INVENTORIES.map((inventory) => inventory.releaseTag),
		).toEqual(["v3.22.5", "v3.21.11", "v3.20.3"]);
		expect(
			RELEASE_INVENTORIES.map((inventory) => inventory.sourceCommit),
		).toEqual([
			"d7e1e652fbd5fa07442bd960894764a5b81ef3bc",
			"8cf9564f366cb260b35106396840cbe3ed903c75",
			"aee855e41f63af0c99f296259b038cda73e24ae9",
		]);
		expect(RELEASE_INVENTORIES[0]?.capabilityProbePath).toBe(
			API_CAPABILITIES_PATH,
		);
		expect(
			RELEASE_INVENTORIES.slice(1).every(
				(inventory) => inventory.capabilityProbePath === undefined,
			),
		).toBe(true);
	});

	it("checks every inventory route against its pinned backend source", () => {
		for (const inventory of RELEASE_INVENTORIES) {
			const source = execFileSync(
				"git",
				["show", `${inventory.sourceCommit}:src/http/mod.rs`],
				{ encoding: "utf8" },
			);
			for (const route of inventory.apiRoutes) {
				const path = route.slice(route.indexOf(" ") + 1);
				const sourcePath = path.startsWith("/api/admin")
					? path.slice("/api/admin".length)
					: path.slice("/api".length);
				expect(source).toContain(`"${sourcePath}"`);
			}
		}
	});

	it("covers every pinned Web API module and every inventoried response", () => {
		const helperModules = new Set([
			"backendError",
			"m1Schemas",
			"mihomoProfileMock",
			"quotaReset",
			"releaseInventories",
		]);
		for (const inventory of RELEASE_INVENTORIES) {
			const files = execFileSync(
				"git",
				[
					"ls-tree",
					"-r",
					"--name-only",
					inventory.sourceCommit,
					":(top)web/src/api",
				],
				{ encoding: "utf8" },
			);
			const modules = files
				.split("\n")
				.filter((file) => file.endsWith(".ts") && !file.endsWith(".test.ts"))
				.map((file) => file.slice(file.lastIndexOf("/") + 1, -3))
				.filter((module) => !helperModules.has(module))
				.sort();
			expect([...inventory.webCallsites].sort()).toEqual(modules);
			expect(Object.keys(inventory.responseSchemas).sort()).toEqual(
				[...inventory.apiRoutes].sort(),
			);
		}
	});

	it("contains immutable per-minor routes and tolerant response contracts", () => {
		for (const inventory of RELEASE_INVENTORIES) {
			expect(inventory.webCallsites).toContain("adminStatusEvents");
			expect(inventory.apiRoutes).toContain("GET /api/admin/status/events");
			expect(inventory.apiRoutes).toContain("POST /api/cluster/join");
			expect(inventory.apiRoutes).toContain(
				"DELETE /api/admin/nodes/{node_id}",
			);
			expect(inventory.apiRoutes).toContain(
				"GET /api/sub/{subscription_token}/mihomo/provider",
			);
			expect(inventory.apiRoutes).not.toContain("GET /events");
			expect(inventory.responseSchemas["GET /api/health"]).toContain("status");
			expect(inventory.fingerprint["/api/health"]).toContain("status");
		}
		const current = RELEASE_INVENTORIES[0];
		const legacy = RELEASE_INVENTORIES[1];
		expect(current?.apiRoutes).not.toContain("GET /api/capabilities");
		expect(current?.apiRoutes).toContain("GET /api/admin/mesh/status");
		expect(legacy?.apiRoutes).not.toContain("GET /api/admin/mesh/status");
		expect(current?.apiRoutes).toContain(
			"PUT /api/admin/mihomo/resource-policy",
		);
		expect(legacy?.apiRoutes).not.toContain(
			"PUT /api/admin/mihomo/resource-policy",
		);
		expect(current?.capabilities).toContain("admin.mihomo-resource-policy");
		expect(legacy?.capabilities).not.toContain("admin.mihomo-resource-policy");
		expect(current?.responseSchemas["GET /api/admin/status/events"]).toContain(
			"mesh_revision",
		);
		expect(
			legacy?.responseSchemas["GET /api/admin/status/events"],
		).not.toContain("mesh_revision");
	});

	it("executes both compatibility directions for every pinned inventory", () => {
		expect(RELEASE_COMPATIBILITY_CONTRACTS).toHaveLength(6);
		expect(RELEASE_API_FIXTURES).toHaveLength(6);
		expect(
			new Set(RELEASE_API_FIXTURES.map((fixture) => fixture.direction)),
		).toEqual(new Set(["new-web-to-legacy-api", "legacy-web-to-new-api"]));
		for (const fixture of RELEASE_API_FIXTURES) {
			const consumer = RELEASE_INVENTORIES.find(
				(candidate) => candidate.releaseTag === fixture.consumerReleaseTag,
			);
			const server = RELEASE_INVENTORIES.find(
				(candidate) => candidate.releaseTag === fixture.serverReleaseTag,
			);
			expect(consumer).toBeDefined();
			expect(server).toBeDefined();
			if (!consumer || !server) continue;
			expect(fixture.consumerSourceCommit).toBe(consumer.sourceCommit);
			expect(fixture.serverSourceCommit).toBe(server.sourceCommit);
			expect(fixture.consumerReleaseTag).toBe(consumer.releaseTag);
			expect(fixture.serverReleaseTag).toBe(server.releaseTag);
			const probe = new Request(
				`https://fixture.invalid${API_CAPABILITIES_PATH}`,
			);
			const probeResponse = new Response(null, {
				status: server.capabilityProbePath ? 200 : 404,
			});
			expect(probe.method).toBe("GET");
			expect(probeResponse.status).toBe(server.capabilityProbePath ? 200 : 404);

			const requestRoutes = fixture.requests.map(
				(request) => `${request.method} ${request.path}`,
			);
			expect(new Set(requestRoutes)).toEqual(new Set(consumer.apiRoutes));
			for (const request of fixture.requests) {
				expect([200, 204, 404]).toContain(request.expectedStatus);
				expect(request.requestSchema).toBeDefined();
				const requestUrl = `https://fixture.invalid${request.path.replaceAll(
					/\{[^}]+\}/g,
					"fixture",
				)}`;
				const executedRequest = new Request(requestUrl, {
					method: request.method,
				});
				expect(
					`${executedRequest.method} ${new URL(executedRequest.url).pathname}`,
				).toBe(
					`${request.method} ${request.path.replaceAll(/\{[^}]+\}/g, "fixture")}`,
				);
				const executedResponse = executeFixtureRequest(
					fixture,
					server.apiRoutes,
					request.route,
					executedRequest,
				);
				expect(executedResponse.status).toBe(request.expectedStatus);
				if (request.expectedStatus === 404) {
					expect(request.optional).toBe(true);
					expect(server.apiRoutes).not.toContain(request.route);
				} else {
					expect(request.optional).toBe(false);
					expect(server.apiRoutes).toContain(request.route);
				}
			}
			for (const route of Object.keys(consumer.responseSchemas)) {
				if (!server.apiRoutes.includes(route)) continue;
				expect(
					fixture.responses.some((response) => response.route === route),
				).toBe(true);
			}

			for (const response of fixture.responses) {
				const fields = response.fields;
				const serializedResponse = new Response(
					response.wireBody ??
						(response.contentType === "application/json"
							? JSON.stringify(response.body)
							: String(response.body)),
					{
						status: 200,
						headers: { "Content-Type": response.contentType },
					},
				);
				expect(serializedResponse.status).toBe(200);
				expect(serializedResponse.headers.get("Content-Type")).toBe(
					response.contentType,
				);
				if (response.contentType === "text/plain") {
					expect(fields).toContain("text/plain");
					continue;
				}
				if (response.contentType === "text/event-stream") {
					const blocks = String(response.wireBody).split("\n\n");
					const events = new Map(
						blocks.filter(Boolean).map((block) => {
							const lines = block.split("\n");
							return [
								lines[0]?.replace("event: ", ""),
								JSON.parse(lines[1]?.replace("data: ", "") ?? "{}"),
							] as const;
						}),
					);
					AdminStatusEventsHelloSchema.parse(events.get("hello"));
					AdminStatusEventsSnapshotSchema.parse(events.get("snapshot"));
				}

				if (response.route === "GET /api/health") {
					HealthResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/cluster/info") {
					ClusterInfoResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/admin/nodes") {
					AdminNodesResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/admin/nodes/runtime") {
					AdminNodesRuntimeResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/version/check") {
					VersionCheckResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/admin/config") {
					AdminConfigResponseSchema.parse(response.body);
				}

				const body = response.body as Record<string, unknown>;
				for (const missingField of response.missingFields) {
					expect(body).not.toHaveProperty(
						response.route === "GET /api/admin/status/events"
							? `snapshot.${missingField}`
							: missingField,
					);
				}
				for (const field of fields) {
					if (response.route === "GET /api/admin/status/events") {
						const eventPath =
							field === "hello" || field === "snapshot"
								? field
								: `snapshot.${field}`;
						expect(body).toHaveProperty(eventPath);
					} else {
						expect(body).toHaveProperty(field);
					}
				}
			}
		}
		const serverBodies = RELEASE_API_FIXTURES.filter(
			(fixture) => fixture.direction === "new-web-to-legacy-api",
		).map(
			(fixture) =>
				fixture.responses.find(
					(response) => response.route === "GET /api/cluster/info",
				)?.body,
		);
		expect(new Set(serverBodies.map((body) => JSON.stringify(body))).size).toBe(
			3,
		);

		const currentToN2 = RELEASE_API_FIXTURES.find(
			(fixture) =>
				fixture.direction === "new-web-to-legacy-api" &&
				fixture.serverReleaseTag === "v3.20.3",
		);
		const statusEvents = currentToN2?.responses.find(
			(response) => response.route === "GET /api/admin/status/events",
		);
		expect(statusEvents?.body).not.toHaveProperty("snapshot.mesh_revision");
		expect(statusEvents?.missingFields).toContain("mesh_revision");

		for (const inventory of RELEASE_INVENTORIES) {
			const newWebContract = RELEASE_COMPATIBILITY_CONTRACTS.find(
				(contract) =>
					contract.direction === "new-web-to-legacy-api" &&
					contract.serverReleaseTag === inventory.releaseTag,
			);
			const legacyWebContract = RELEASE_COMPATIBILITY_CONTRACTS.find(
				(contract) =>
					contract.direction === "legacy-web-to-new-api" &&
					contract.consumerReleaseTag === inventory.releaseTag,
			);
			expect(newWebContract?.serverRoutes).toEqual(inventory.apiRoutes);
			expect(legacyWebContract?.consumerRoutes).toEqual(inventory.apiRoutes);
			expect(newWebContract?.webCallsites).toEqual(
				RELEASE_INVENTORIES[0]?.webCallsites,
			);
			expect(legacyWebContract?.webCallsites).toEqual(inventory.webCallsites);
			expect(newWebContract?.requestSchemas).toBeDefined();
			expect(legacyWebContract?.requestSchemas).toEqual(
				inventory.requestSchemas,
			);
		}
	});
});
