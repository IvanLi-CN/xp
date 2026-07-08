import { useRegisterSW } from "virtual:pwa-register/react";
import { useEffect, useRef } from "react";

import { startServiceWorkerUpdatePolling } from "../offline/serviceWorkerUpdates";
import { PwaUpdateNotice } from "./PwaUpdateNotice";

export function PwaStatusPrompt() {
	const stopPollingRef = useRef<(() => void) | null>(null);

	useEffect(() => {
		return () => {
			if (stopPollingRef.current) {
				stopPollingRef.current();
				stopPollingRef.current = null;
			}
		};
	}, []);

	const {
		offlineReady: [, setOfflineReady],
		needRefresh: [needRefresh, setNeedRefresh],
		updateServiceWorker,
	} = useRegisterSW({
		onRegisteredSW(_swUrl, registration) {
			if (stopPollingRef.current) {
				stopPollingRef.current();
			}
			stopPollingRef.current = startServiceWorkerUpdatePolling(
				registration,
				__XP_WEB_SW_UPDATE_INTERVAL_MS__,
			);
		},
		onRegisterError(error) {
			console.error("Failed to register service worker", error);
		},
	});

	if (!needRefresh) {
		return null;
	}

	return (
		<PwaUpdateNotice
			onClose={() => {
				setOfflineReady(false);
				setNeedRefresh(false);
			}}
			onReload={() => {
				void updateServiceWorker(true);
			}}
		/>
	);
}
