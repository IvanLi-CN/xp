import { afterEach, describe, expect, it } from "vitest";

import {
	acquireMeshMutationLock,
	clearAllMeshMutationLocksForTests,
	clearMeshMutationLock,
	readMeshMutationLock,
} from "./meshMutationLock";

describe("mesh mutation lock", () => {
	afterEach(() => {
		clearAllMeshMutationLocksForTests();
	});

	it("persists a target per cluster and does not clear from status observations", () => {
		const lock = {
			clusterId: "cluster-1",
			targetEnabled: true,
			createdAt: 123,
		};

		expect(acquireMeshMutationLock(lock)).toBe(true);
		expect(readMeshMutationLock("cluster-1")).toEqual(lock);
		expect(readMeshMutationLock("cluster-2")).toBeNull();
	});

	it("clears only after explicit local unlock", () => {
		expect(
			acquireMeshMutationLock({
				clusterId: "cluster-1",
				targetEnabled: false,
				createdAt: 456,
			}),
		).toBe(true);
		expect(clearMeshMutationLock("cluster-1")).toBe(true);
		expect(readMeshMutationLock("cluster-1")).toBeNull();
	});
});
