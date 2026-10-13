import { afterEach, describe, expect, it } from "vitest";

import {
	getDemoSnapshot,
	handleDemoRequest,
	syncDemoControls,
} from "./runtime";

const demoUrl = "https://demo.example/api/admin/users";

afterEach(() => {
	syncDemoControls("?demo=normal&d_persona=admin&d_network=online", true);
});

describe("demo runtime", () => {
	it("simulates anonymous administrator requests without using live auth", async () => {
		syncDemoControls("?demo=normal&d_persona=anonymous", true);

		const response = await handleDemoRequest(new Request(demoUrl));

		expect(response.status).toBe(401);
		expect(await response.json()).toMatchObject({
			error: { code: "unauthorized" },
		});
	});

	it("keeps an in-flight request on the model that started it", async () => {
		syncDemoControls("?demo=normal&d_network=slow", true);
		const pending = handleDemoRequest(new Request(demoUrl));
		syncDemoControls("?demo=empty&d_network=online", true);

		const response = await pending;
		const body = (await response.json()) as { items: unknown[] };

		expect(body.items.length).toBeGreaterThan(0);
	});

	it("keeps simulated faults and unknown routes inside the mock boundary", async () => {
		syncDemoControls("?demo=normal&d_network=error", true);
		const failed = await handleDemoRequest(new Request(demoUrl));
		expect(failed.status).toBe(503);

		syncDemoControls("?demo=normal&d_network=online", true);
		const unknown = await handleDemoRequest(
			new Request("https://demo.example/api/demo/unknown"),
		);
		expect(unknown.status).toBe(404);
	});

	it("redacts unknown mutation paths in the operation journal", async () => {
		syncDemoControls("?demo=normal&d_network=online", true);
		await handleDemoRequest(
			new Request("https://demo.example/api/custom/secret-token", {
				method: "POST",
			}),
		);

		expect(getDemoSnapshot().operations[0]?.route).toBe("/api/:route");
	});

	it("keeps approved reset actions identifiable without user IDs", async () => {
		syncDemoControls("?demo=normal&d_network=online", true);
		await handleDemoRequest(
			new Request(
				"https://demo.example/api/admin/users/user-fixture-c/reset-token",
				{
					method: "POST",
				},
			),
		);

		expect(getDemoSnapshot().operations[0]?.route).toBe(
			"/api/admin/users/:id/reset-token",
		);
	});
});
