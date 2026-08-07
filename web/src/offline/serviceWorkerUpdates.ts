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
