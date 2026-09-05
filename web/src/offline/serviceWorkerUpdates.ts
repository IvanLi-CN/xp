export async function requestServiceWorkerUpdateCheck(
	registration?: Pick<ServiceWorkerRegistration, "update"> | null,
) {
	const target =
		registration ??
		(typeof navigator === "undefined" || !("serviceWorker" in navigator)
			? null
			: await navigator.serviceWorker.getRegistration());
	if (!target) return false;

	await target.update();
	return true;
}

type ServiceWorkerUpdateRegistration = Pick<
	ServiceWorkerRegistration,
	"active" | "waiting"
>;

type ReloadAfterUpdateOptions = {
	registration?: ServiceWorkerUpdateRegistration | null;
	reload?: () => void;
	timeoutMs?: number;
	pollIntervalMs?: number;
};

/**
 * Completes the user-confirmed PWA update without depending on clients.claim().
 * Normal XP updates intentionally leave existing pages controlled by the old
 * worker, so the page must reload after the waiting worker has activated.
 */
export async function reloadAfterServiceWorkerUpdate(
	updateServiceWorker: (reloadPage?: boolean) => Promise<unknown>,
	options: ReloadAfterUpdateOptions = {},
): Promise<"failed" | "reloaded" | "timed_out"> {
	const reload =
		options.reload ??
		(() => {
			if (typeof window !== "undefined") window.location.reload();
		});
	let registration = options.registration;
	if (registration === undefined) {
		try {
			registration =
				typeof navigator === "undefined" || !("serviceWorker" in navigator)
					? null
					: await navigator.serviceWorker.getRegistration();
		} catch {
			registration = null;
		}
	}

	// A prompt can outlive the waiting worker when another tab activates it.
	// Refreshing is the only way to move this old document to the active build.
	if (!registration?.waiting) {
		reload();
		return "reloaded";
	}

	const waitingWorker = registration.waiting;
	try {
		await updateServiceWorker(true);
	} catch {
		return "failed";
	}

	const timeoutMs = Math.max(0, options.timeoutMs ?? 10_000);
	const pollIntervalMs = Math.max(1, options.pollIntervalMs ?? 50);
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (
			registration.waiting === null &&
			registration.active === waitingWorker &&
			waitingWorker.state === "activated"
		) {
			reload();
			return "reloaded";
		}
		await new Promise<void>((resolve) => {
			window.setTimeout(resolve, pollIntervalMs);
		});
	}
	return "timed_out";
}

function scheduleServiceWorkerUpdateCheck(
	registration: Pick<ServiceWorkerRegistration, "update">,
) {
	void requestServiceWorkerUpdateCheck(registration).catch(() => {
		// Update checks are best-effort; an offline or transient failure must not
		// become an unhandled rejection during page startup.
	});
}

export function startServiceWorkerUpdatePolling(
	registration: Pick<ServiceWorkerRegistration, "update"> | null | undefined,
	intervalMs: number,
) {
	if (!registration) {
		return () => {};
	}

	scheduleServiceWorkerUpdateCheck(registration);
	if (intervalMs <= 0) {
		return () => {};
	}

	const intervalId = window.setInterval(() => {
		scheduleServiceWorkerUpdateCheck(registration);
	}, intervalMs);

	return () => {
		window.clearInterval(intervalId);
	};
}
