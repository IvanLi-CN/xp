import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	requestServiceWorkerUpdateCheck,
	startServiceWorkerUpdatePolling,
} from "./serviceWorkerUpdates";

describe("serviceWorkerUpdates", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it("updates an explicit registration without requiring navigator.serviceWorker", async () => {
		const update = vi.fn().mockResolvedValue(undefined);

		await expect(
			requestServiceWorkerUpdateCheck({
				update,
			} as Pick<ServiceWorkerRegistration, "update">),
		).resolves.toBe(true);
		expect(update).toHaveBeenCalledTimes(1);
	});

	it("checks immediately and then polls the registered service worker", async () => {
		const update = vi.fn().mockResolvedValue(undefined);
		const stop = startServiceWorkerUpdatePolling(
			{ update } as Pick<ServiceWorkerRegistration, "update">,
			60_000,
		);

		expect(update).toHaveBeenCalledTimes(1);

		await vi.advanceTimersByTimeAsync(60_000);
		expect(update).toHaveBeenCalledTimes(2);

		stop();
		await vi.advanceTimersByTimeAsync(60_000);
		expect(update).toHaveBeenCalledTimes(2);
	});
});
