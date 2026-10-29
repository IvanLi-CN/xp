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

export type MeshMutationLockContext = {
	clear: () => boolean;
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
		const current = readAll();
		if (current === null) return false;
		const next = { ...current, [lock.clusterId]: lock };
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

function readAll(): Record<string, MeshMutationLock> | null {
	const store = storage();
	if (!store) return null;
	try {
		const raw = store.getItem(STORAGE_KEY);
		if (raw === null) return {};
		const parsed: unknown = JSON.parse(raw);
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
			return null;
		const result: Record<string, MeshMutationLock> = {};
		for (const [clusterId, value] of Object.entries(parsed)) {
			if (
				typeof value === "object" &&
				value !== null &&
				(value as MeshMutationLock).clusterId === clusterId &&
				typeof (value as MeshMutationLock).clusterId === "string" &&
				typeof (value as MeshMutationLock).targetEnabled === "boolean" &&
				typeof (value as MeshMutationLock).createdAt === "number"
			) {
				result[clusterId] = value as MeshMutationLock;
				continue;
			}
			return null;
		}
		return result;
	} catch {
		return null;
	}
}

export function readMeshMutationLock(
	clusterId: string | null | undefined,
): MeshMutationLock | null {
	if (!clusterId) return null;
	return readAll()?.[clusterId] ?? null;
}

export function readMeshMutationLocks(): MeshMutationLock[] {
	return Object.values(readAll() ?? {});
}

/**
 * Persist the lock before running the mutation while holding a browser-wide
 * exclusive lock. The Web Locks API closes the read-modify-write race between
 * tabs; unsupported browsers fail closed and never run the operation.
 */
export async function withMeshMutationLock<T>(
	lock: MeshMutationLock,
	operation: (context: MeshMutationLockContext) => Promise<T>,
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
			return {
				acquired: true,
				value: await operation({
					clear: () => clearMeshMutationLockValue(lock.clusterId),
				}),
			};
		},
	);
}

function clearMeshMutationLockValue(clusterId: string): boolean {
	const store = storage();
	if (!store) return false;
	try {
		const next = readAll();
		if (next === null) return false;
		delete next[clusterId];
		store.setItem(STORAGE_KEY, JSON.stringify(next));
		return readMeshMutationLock(clusterId) === null;
	} catch {
		return false;
	}
}

export async function clearMeshMutationLock(
	clusterId: string,
): Promise<boolean> {
	if (typeof navigator === "undefined" || !navigator.locks) return false;
	return navigator.locks.request(
		WEB_LOCK_NAME,
		{ mode: "exclusive", ifAvailable: true },
		async (heldLock) =>
			heldLock ? clearMeshMutationLockValue(clusterId) : false,
	);
}

export function clearAllMeshMutationLocksForTests(): void {
	const store = storage();
	try {
		store?.removeItem(STORAGE_KEY);
	} catch {
		// Tests and private browsing may not expose writable storage.
	}
}
