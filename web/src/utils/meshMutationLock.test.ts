import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";

import {
	clearAllMeshMutationLocksForTests,
	clearMeshMutationLock,
	readMeshMutationLock,
	withMeshMutationLock,
} from "./meshMutationLock";

describe("mesh mutation lock", () => {
	afterEach(() => {
		clearAllMeshMutationLocksForTests();
		vi.unstubAllGlobals();
	});

	it("persists a target per cluster while the mutation is running", async () => {
		vi.stubGlobal("navigator", {
			locks: {
				request: async (
					_name: string,
					_options: LockOptions,
					callback: (lock: Lock | null) => Promise<unknown>,
				) => callback({ name: fixtureCatalog.cluster.fixture53() } as Lock),
			},
		});
		const lock = {
			clusterId: fixtureCatalog.cluster.fixture53(),
			targetEnabled: true,
			createdAt: fixtureCatalog.number.value123(),
		};

		const result = await withMeshMutationLock(lock, async () => {
			expect(readMeshMutationLock(fixtureCatalog.cluster.fixture53())).toEqual(
				lock,
			);
			return "completed";
		});

		expect(result).toEqual({ acquired: true, value: "completed" });
		expect(clearMeshMutationLock(fixtureCatalog.cluster.fixture53())).toBe(
			true,
		);
	});

	it("rejects a concurrent tab before it can send a second mutation", async () => {
		let active = false;
		let releaseFirst: (() => void) | undefined;
		vi.stubGlobal("navigator", {
			locks: {
				request: async (
					_name: string,
					_options: LockOptions,
					callback: (lock: Lock | null) => Promise<unknown>,
				) => {
					if (active) return callback(null);
					active = true;
					try {
						return await callback({
							name: fixtureCatalog.cluster.fixture53(),
						} as Lock);
					} finally {
						active = false;
					}
				},
			},
		});
		const lock = {
			clusterId: fixtureCatalog.cluster.fixture53(),
			targetEnabled: false,
			createdAt: fixtureCatalog.number.value124(),
		};

		const first = withMeshMutationLock(
			lock,
			() =>
				new Promise<string>((resolve) => {
					releaseFirst = () => resolve("completed");
				}),
		);
		await new Promise((resolve) => setTimeout(resolve, 0));
		const second = await withMeshMutationLock(lock, async () => "unexpected");

		expect(second).toEqual({ acquired: false, reason: "held" });
		releaseFirst?.();
		expect(await first).toEqual({ acquired: true, value: "completed" });
	});
});
