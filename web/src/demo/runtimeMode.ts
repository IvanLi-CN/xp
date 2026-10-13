/** Demo is an explicit build/runtime, never selected by a live query parameter. */
export function isDemoRuntime(): boolean {
	return (
		(typeof __XP_WEB_DEMO__ !== "undefined" && __XP_WEB_DEMO__) ||
		(import.meta.env.DEV &&
			typeof window !== "undefined" &&
			/^\/demo(?:\/|$)/.test(window.location.pathname))
	);
}

let demoAuthenticated = true;

export function readDemoAuthentication(): boolean {
	return demoAuthenticated;
}

export function setDemoAuthentication(authenticated: boolean): void {
	demoAuthenticated = authenticated;
}
