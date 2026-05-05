import { describe, expect, it, vi } from "vitest";

import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	SUBSCRIPTION_FORMAT_OPTIONS,
} from "@/api/subscription";

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
		const user = getDemoUser(state, "user-sato");

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
		const user = getDemoUser(state, "user-sato");

		const pending = fetchDemoSubscription(state, user, "mihomo");
		await vi.advanceTimersByTimeAsync(240);

		await expect(pending).resolves.toContain("# provider mode preview");
		vi.useRealTimers();
	});
});
