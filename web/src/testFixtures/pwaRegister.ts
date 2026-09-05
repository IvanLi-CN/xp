export function useRegisterSW() {
	return {
		offlineReady: [false, () => {}] as const,
		needRefresh: [false, () => {}] as const,
		updateServiceWorker: async () => {},
	};
}
