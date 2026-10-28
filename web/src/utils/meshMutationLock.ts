export type MeshMutationLock = {
	clusterId: string;
	targetEnabled: boolean;
	createdAt: number;
};

const STORAGE_KEY = "xp-mesh-config-mutation-locks";

function storage(): Storage | null {
	if (typeof window === "undefined") return null;
	try {
		return window.localStorage;
	} catch {
		return null;
	}
}

function readAll(): Record<string, MeshMutationLock> {
	const store = storage();
	if (!store) return {};
	try {
		const parsed: unknown = JSON.parse(store.getItem(STORAGE_KEY) ?? "{}");
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
			return {};
		const result: Record<string, MeshMutationLock> = {};
		for (const [clusterId, value] of Object.entries(parsed)) {
			if (
				typeof value === "object" &&
				value !== null &&
				typeof (value as MeshMutationLock).clusterId === "string" &&
				typeof (value as MeshMutationLock).targetEnabled === "boolean" &&
				typeof (value as MeshMutationLock).createdAt === "number"
			) {
				result[clusterId] = value as MeshMutationLock;
			}
		}
		return result;
	} catch {
		return {};
	}
}

export function readMeshMutationLock(
	clusterId: string | null | undefined,
): MeshMutationLock | null {
	if (!clusterId) return null;
	return readAll()[clusterId] ?? null;
}

export function readMeshMutationLocks(): MeshMutationLock[] {
	return Object.values(readAll());
}

/**
 * The mutation must not be sent unless the browser can prove this lock was
 * persisted. A localStorage write that cannot be read back fails closed.
 */
export function acquireMeshMutationLock(lock: MeshMutationLock): boolean {
	const store = storage();
	if (!store || !lock.clusterId) return false;
	try {
		const next = { ...readAll(), [lock.clusterId]: lock };
		store.setItem(STORAGE_KEY, JSON.stringify(next));
		const persisted = readMeshMutationLock(lock.clusterId);
		return (
			persisted?.clusterId === lock.clusterId &&
			persisted.targetEnabled === lock.targetEnabled &&
			persisted.createdAt === lock.createdAt
		);
	} catch {
		return false;
	}
}

export function clearMeshMutationLock(clusterId: string): boolean {
	const store = storage();
	if (!store) return false;
	try {
		const next = readAll();
		delete next[clusterId];
		store.setItem(STORAGE_KEY, JSON.stringify(next));
		return readMeshMutationLock(clusterId) === null;
	} catch {
		return false;
	}
}

export function clearAllMeshMutationLocksForTests(): void {
	const store = storage();
	try {
		store?.removeItem(STORAGE_KEY);
	} catch {
		// Tests and private browsing may not expose writable storage.
	}
}
