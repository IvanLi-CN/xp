const mihomoSaveQueues = new Map<string, Promise<void>>();

export function enqueueMihomoSave<T>(
	saveKey: string,
	lockKey: string,
	operation: () => Promise<T>,
): Promise<T> {
	const previous = mihomoSaveQueues.get(saveKey) ?? Promise.resolve();
	const run = async () => {
		if (typeof navigator !== "undefined" && navigator.locks) {
			return await navigator.locks.request<Promise<T>>(
				`xp-mihomo-profile:${lockKey}`,
				operation,
			);
		}
		return operation();
	};
	const current = previous.catch(() => undefined).then(run);
	const settled = current.then(
		() => undefined,
		() => undefined,
	);
	mihomoSaveQueues.set(saveKey, settled);
	return current.finally(() => {
		if (mihomoSaveQueues.get(saveKey) === settled) {
			mihomoSaveQueues.delete(saveKey);
		}
	});
}
