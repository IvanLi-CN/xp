export type MeshMutationLock = {
	clusterId: string;
	targetEnabled: boolean;
	createdAt: number;
};

export type MeshMutationLockRunResult<T> =
	| { acquired: true; value: T }
	| {
			acquired: false;
			reason: "held" | "persistence" | "unavailable";
	  };

const STORAGE_KEY = "xp-mesh-config-mutation-locks";
// The persisted value is one shared cluster-id map. A single Web Lock protects
// its read-modify-write lifecycle across different cluster ids as well.
const WEB_LOCK_NAME = "xp-mesh-config-mutation-storage";

function storage(): Storage | null {
	if (typeof window === "undefined") return null;
	try {
		return window.localStorage;
	} catch {
		return null;
	}
}

function persistLock(lock: MeshMutationLock): boolean {
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
 * Persist the lock before running the mutation while holding a browser-wide
 * exclusive lock. The Web Locks API closes the read-modify-write race between
 * tabs; unsupported browsers fail closed and never run the operation.
 */
export async function withMeshMutationLock<T>(
	lock: MeshMutationLock,
	operation: () => Promise<T>,
): Promise<MeshMutationLockRunResult<T>> {
	if (typeof navigator === "undefined" || !navigator.locks) {
		return { acquired: false, reason: "unavailable" };
	}
	return navigator.locks.request(
		WEB_LOCK_NAME,
		{ mode: "exclusive", ifAvailable: true },
		async (heldLock) => {
			if (!heldLock || readMeshMutationLock(lock.clusterId)) {
				return { acquired: false, reason: "held" };
			}
			if (!persistLock(lock)) {
				return { acquired: false, reason: "persistence" };
			}
			return { acquired: true, value: await operation() };
		},
	);
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
