import { useCallback, useEffect, useState } from "react";

import type { UpgradeJobStatus } from "@/api/adminUpgrade";

import {
	type UpgradeObservation,
	beginUpgradeObservation,
	observeUpgradeStatus,
	readUpgradeObservation,
	refreshTimedOutObservation,
	writeUpgradeObservation,
} from "./upgradeObservation";

function updateObservation(
	setObservation: (next: UpgradeObservation | null) => void,
	next: UpgradeObservation | null,
) {
	writeUpgradeObservation(next);
	setObservation(next);
}

export function useUpgradeObservation() {
	const [observation, setObservation] = useState<UpgradeObservation | null>(
		() => readUpgradeObservation(Date.now()),
	);

	const begin = useCallback((targetTag: string) => {
		updateObservation(
			setObservation,
			beginUpgradeObservation(targetTag, Date.now()),
		);
	}, []);

	const observeStatus = useCallback((status: UpgradeJobStatus | null) => {
		setObservation((current) => {
			const next = observeUpgradeStatus(current, status, Date.now());
			if (next !== current) writeUpgradeObservation(next);
			return next;
		});
	}, []);

	const refreshTimedOutStatus = useCallback(
		(status: UpgradeJobStatus | null) => {
			setObservation((current) => {
				const next = refreshTimedOutObservation(current, status, Date.now());
				if (next !== current) writeUpgradeObservation(next);
				return next;
			});
		},
		[],
	);

	const clear = useCallback(() => updateObservation(setObservation, null), []);

	useEffect(() => {
		if (!observation || observation.phase !== "observing") return;
		const timeoutMs = Math.max(0, observation.deadlineAtMs - Date.now());
		const timer = window.setTimeout(() => {
			setObservation((current) => {
				const next = observeUpgradeStatus(current, null, Date.now());
				if (next !== current) writeUpgradeObservation(next);
				return next;
			});
		}, timeoutMs);
		return () => window.clearTimeout(timer);
	}, [observation]);

	return {
		observation,
		isObserving: observation?.phase === "observing",
		begin,
		clear,
		observeStatus,
		refreshTimedOutStatus,
	};
}
