import { describe, expect, it, vi } from "vitest";

import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	SUBSCRIPTION_FORMAT_OPTIONS,
} from "@/api/subscription";
import { fixtureCatalog } from "@/fixture-policy/catalog";

import { createDemoState } from "./fixtures";
import { fetchDemoSubscription } from "./mockApi";
import type { DemoState, DemoUser } from "./types";

function getDemoUser(state: DemoState, userId: string): DemoUser {
	const user = state.users.find((item) => item.id === userId);
	if (!user) throw new Error(`Missing demo user: ${userId}`);
	return user;
}

describe("demo mock API", () => {
	it("uses the shared subscription format contract", async () => {
		vi.useFakeTimers();
		const state = createDemoState("normal");
		const user = getDemoUser(state, fixtureCatalog.identifier.userTertiary());

		const pending = fetchDemoSubscription(
			state,
			user,
			DEFAULT_SUBSCRIPTION_FORMAT,
		);
		await vi.advanceTimersByTimeAsync(240);

		expect(await pending).toContain("vless://");
		expect(SUBSCRIPTION_FORMAT_OPTIONS.map((option) => option.value)).toEqual([
			"raw",
			"clash",
			"mihomo",
		]);
		vi.useRealTimers();
	});

	it("returns provider-mode Mihomo output for format=mihomo", async () => {
		vi.useFakeTimers();
		const state = createDemoState("normal");
		const user = getDemoUser(state, fixtureCatalog.identifier.userTertiary());

		const pending = fetchDemoSubscription(state, user, "mihomo");
		await vi.advanceTimersByTimeAsync(240);

		await expect(pending).resolves.toContain("# provider mode preview");
		await expect(pending).resolves.toContain("reality-opts:");
		await expect(pending).resolves.toContain("proxy-groups:");
		await expect(pending).resolves.toContain("patterns:");
		vi.useRealTimers();
	});

	it("shows rewritten external resources in Mihomo mirror mode", async () => {
		vi.useFakeTimers();
		const state = createDemoState("normal");
		const user = getDemoUser(state, fixtureCatalog.identifier.userTertiary());

		const pending = fetchDemoSubscription(state, user, "mihomo", true);
		await vi.advanceTimersByTimeAsync(240);

		await expect(pending).resolves.toContain("# XP mirror mode");
		await expect(pending).resolves.toContain(
			"https://xp.example.net/api/mihomo/resources/demo-rules",
		);
		await expect(pending).resolves.toContain("proxy: DIRECT");
		vi.useRealTimers();
	});

	it("rejects mirror mode without a saved Mihomo profile", async () => {
		vi.useFakeTimers();
		const state = createDemoState("normal");
		const user = getDemoUser(state, fixtureCatalog.identifier.userQuinary());

		const pending = fetchDemoSubscription(state, user, "mihomo", true);
		const rejected = expect(pending).rejects.toThrow("saved Mihomo profile");
		await vi.advanceTimersByTimeAsync(240);

		await rejected;
		vi.useRealTimers();
	});
});
