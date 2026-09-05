import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	reloadAfterServiceWorkerUpdate,
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

	it("checks immediately when periodic polling is disabled", () => {
		const update = vi.fn().mockResolvedValue(undefined);
		const stop = startServiceWorkerUpdatePolling(
			{ update } as Pick<ServiceWorkerRegistration, "update">,
			0,
		);

		expect(update).toHaveBeenCalledTimes(1);
		vi.advanceTimersByTime(60_000);
		expect(update).toHaveBeenCalledTimes(1);
		stop();
	});

	it("keeps rejected best-effort checks from becoming unhandled rejections", async () => {
		const update = vi.fn().mockRejectedValue(new Error("offline"));
		const stop = startServiceWorkerUpdatePolling(
			{ update } as Pick<ServiceWorkerRegistration, "update">,
			0,
		);

		await vi.advanceTimersByTimeAsync(0);
		expect(update).toHaveBeenCalledTimes(1);
		stop();
	});

	it("reloads immediately when the update prompt has become stale", async () => {
		const updateServiceWorker = vi.fn().mockResolvedValue(undefined);
		const reload = vi.fn();

		await expect(
			reloadAfterServiceWorkerUpdate(updateServiceWorker, {
				registration: { active: null, waiting: null },
				reload,
			}),
		).resolves.toBe("reloaded");

		expect(updateServiceWorker).not.toHaveBeenCalled();
		expect(reload).toHaveBeenCalledOnce();
	});

	it("waits for a waiting worker to activate before reloading", async () => {
		const waitingWorker = {
			state: "installed",
		} as ServiceWorker;
		let active: ServiceWorker | null = null;
		let waiting: ServiceWorker | null = waitingWorker;
		const updateServiceWorker = vi.fn().mockImplementation(async () => {
			setTimeout(() => {
				waiting = null;
				active = waitingWorker;
				Object.assign(waitingWorker, { state: "activated" });
			}, 100);
		});
		const reload = vi.fn();

		const result = reloadAfterServiceWorkerUpdate(updateServiceWorker, {
			registration: {
				get active() {
					return active;
				},
				get waiting() {
					return waiting;
				},
			},
			reload,
			pollIntervalMs: 50,
			timeoutMs: 1_000,
		});

		expect(reload).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(100);
		await expect(result).resolves.toBe("reloaded");

		expect(updateServiceWorker).toHaveBeenCalledWith(true);
		expect(reload).toHaveBeenCalledOnce();
	});
});
