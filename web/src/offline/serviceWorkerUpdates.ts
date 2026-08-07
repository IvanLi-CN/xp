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

export function startServiceWorkerUpdatePolling(
	registration: Pick<ServiceWorkerRegistration, "update"> | null | undefined,
	intervalMs: number,
) {
	if (!registration) {
		return () => {};
	}

	void requestServiceWorkerUpdateCheck(registration);
	if (intervalMs <= 0) {
		return () => {};
	}

	const intervalId = window.setInterval(() => {
		void requestServiceWorkerUpdateCheck(registration);
	}, intervalMs);

	return () => {
		window.clearInterval(intervalId);
	};
}
