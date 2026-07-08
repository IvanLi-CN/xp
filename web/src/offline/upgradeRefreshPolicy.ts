const ACTIVE_UPGRADE_STATES = new Set(["running", "restarting"]);
const TERMINAL_UPGRADE_STATES = new Set(["succeeded", "failed", "unsupported"]);

export function shouldRefreshAfterUpgradeTransition(
	previousState: string | null,
	nextState: string | null,
) {
	if (!previousState || !nextState) {
		return false;
	}

	return (
		ACTIVE_UPGRADE_STATES.has(previousState) &&
		TERMINAL_UPGRADE_STATES.has(nextState)
	);
}
