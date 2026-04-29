import type { DemoSession, DemoState } from "./types";

const DEMO_STORAGE_KEY = "xp_demo_state";

let fallbackState: DemoState | null = null;
let preferFallbackState = false;

export function isDemoSession(
	value: DemoState["session"] | undefined,
): value is DemoSession {
	return (
		value !== null &&
		value !== undefined &&
		(value.role === "admin" ||
			value.role === "operator" ||
			value.role === "viewer") &&
		typeof value.operatorName === "string" &&
		typeof value.startedAt === "string"
	);
}

export function getDemoStorageKey() {
	return DEMO_STORAGE_KEY;
}

export function readDemoFallbackState() {
	return fallbackState;
}

export function setDemoFallbackState(state: DemoState | null) {
	fallbackState = state;
}

export function shouldPreferDemoFallbackState() {
	return preferFallbackState;
}

export function setPreferDemoFallbackState(value: boolean) {
	preferFallbackState = value;
}

export function hasDemoSession(): boolean {
	if (preferFallbackState && fallbackState) {
		return isDemoSession(fallbackState.session);
	}

	try {
		const raw = localStorage.getItem(DEMO_STORAGE_KEY);
		if (!raw) return false;
		const parsed = JSON.parse(raw) as Partial<DemoState>;
		return isDemoSession(parsed.session);
	} catch {
		return fallbackState ? isDemoSession(fallbackState.session) : false;
	}
}

export function clearDemoFallbackState(): void {
	fallbackState = null;
	preferFallbackState = false;
}
