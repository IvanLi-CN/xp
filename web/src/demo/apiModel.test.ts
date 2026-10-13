import { describe, expect, it } from "vitest";

import { createDemoApi } from "./apiModel";
import { DEFAULT_DEMO_CONTROLS } from "./shareState";

describe("demo API model", () => {
	it("keeps mesh config writes consistent with later status reads", async () => {
		const api = createDemoApi(DEFAULT_DEMO_CONTROLS);
		const url = "https://demo.example/api/admin/mesh/config";
		const write = await api.handle(
			new Request(url, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ enabled: false }),
			}),
		);
		const read = await api.handle(
			new Request("https://demo.example/api/admin/mesh/status"),
		);

		expect(await write.json()).toEqual({ enabled: false });
		expect((await read.json()).cluster_mesh_enabled).toBe(false);
	});
});
