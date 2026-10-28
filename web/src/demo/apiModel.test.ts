import { describe, expect, it, vi } from "vitest";

import { AdminNodeTrafficResponseSchema } from "../api/adminTraffic";
import {
	type AdminUser,
	AdminUserSchema,
	AdminUserTokenResponseSchema,
	AdminUsersResponseSchema,
} from "../api/adminUsers";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { createDemoApi } from "./apiModel";
import { DEFAULT_DEMO_CONTROLS } from "./shareState";

describe("demo API model", () => {
	it("creates distinct users and resets tokens without exhausting seeded identities", async () => {
		const api = createDemoApi(DEFAULT_DEMO_CONTROLS);
		const baseUrl = "https://demo.example/api/admin/users";
		const seed = AdminUsersResponseSchema.parse(
			await (await api.handle(new Request(baseUrl))).json(),
		).items;
		const userIds = new Set(seed.map((user) => user.user_id));
		const tokens = new Set(seed.map((user) => user.subscription_token));
		const payload = { display_name: seed[0].display_name };
		const createRequest = () =>
			new Request(baseUrl, {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify(payload),
			});
		let firstCreated: AdminUser | undefined;
		for (let index = 0; index < 4; index += 1) {
			const response = await api.handle(createRequest());
			expect(response.ok).toBe(true);
			const created = AdminUserSchema.parse(await response.json());
			firstCreated ??= created;
			expect(userIds.has(created.user_id)).toBe(false);
			expect(tokens.has(created.subscription_token)).toBe(false);
			userIds.add(created.user_id);
			tokens.add(created.subscription_token);
			const userUrl = `${baseUrl}/${created.user_id}`;
			expect(await (await api.handle(new Request(userUrl))).json()).toEqual(
				created,
			);
			for (let reset = 0; reset < 2; reset += 1) {
				const resetResponse = await api.handle(
					new Request(`${userUrl}/reset-token`, { method: "POST" }),
				);
				expect(resetResponse.ok).toBe(true);
				const token = AdminUserTokenResponseSchema.parse(
					await resetResponse.json(),
				).subscription_token;
				expect(tokens.has(token)).toBe(false);
				tokens.add(token);
				expect(
					(await (await api.handle(new Request(userUrl))).json())
						.subscription_token,
				).toBe(token);
			}
		}
		const users = AdminUsersResponseSchema.parse(
			await (await api.handle(new Request(baseUrl))).json(),
		).items;
		expect(users).toHaveLength(seed.length + 4);
		expect(
			users.filter((user) =>
				seed.some((original) => original.user_id === user.user_id),
			),
		).toEqual(seed);
		const fresh = createDemoApi(DEFAULT_DEMO_CONTROLS);
		expect(await (await fresh.handle(createRequest())).json()).toEqual(
			firstCreated,
		);
	});

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

	it("rejects unsupported Mihomo mutations", async () => {
		const api = createDemoApi(DEFAULT_DEMO_CONTROLS);
		const response = await api.handle(
			new Request("https://demo.example/api/mihomo/resources/demo-resource", {
				method: "POST",
			}),
		);

		expect(response.status).toBe(405);
	});

	it("refreshes normal traffic fixtures after a five-minute bucket advances", async () => {
		const clock = vi.spyOn(Date, "now");
		clock.mockReturnValue(Date.parse("2026-10-28T17:29:59Z"));
		const api = createDemoApi(DEFAULT_DEMO_CONTROLS);
		const url = new URL(
			`/api/admin/nodes/${fixtureCatalog.nodeId.fixture32()}/traffic?window=24h`,
			"https://demo.example",
		);

		try {
			clock.mockReturnValue(Date.parse("2026-10-28T17:30:01Z"));
			const response = AdminNodeTrafficResponseSchema.parse(
				await (await api.handle(new Request(url))).json(),
			);

			expect(response.traffic.window_end_at).toBe("2026-10-28T17:30:00.000Z");
			expect(response.traffic.last_sample_at).toBe("2026-10-28T17:30:00.000Z");
			expect(response.traffic.partial).toBe(false);
		} finally {
			clock.mockRestore();
		}
	});
});
