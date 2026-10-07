import { afterEach, describe, expect, it } from "vitest";

import { enqueueMihomoSave } from "./mihomoSaveQueue";

describe("mihomoSaveQueue", () => {
	const originalLocks = navigator.locks;

	afterEach(() => {
		Object.defineProperty(navigator, "locks", {
			configurable: true,
			value: originalLocks,
		});
	});

	it("fails closed in a browser without Web Locks", async () => {
		Object.defineProperty(navigator, "locks", {
			configurable: true,
			value: undefined,
		});

		await expect(
			enqueueMihomoSave("test-save", "test-user", async () => "saved"),
		).rejects.toThrow("browser locking support");
	});
});
