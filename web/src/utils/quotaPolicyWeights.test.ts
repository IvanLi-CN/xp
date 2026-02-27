import { describe, expect, it } from "vitest";

import {
	RATIO_BASIS_POINTS,
	basisPointsToWeights,
	parsePercentInput,
	rebalanceAfterEdit,
	weightsToBasisPoints,
} from "./quotaPolicyWeights";

describe("quotaPolicyWeights", () => {
	it("normalizes weights to 10000 basis points", () => {
		expect(weightsToBasisPoints([40, 60])).toEqual([4000, 6000]);
		expect(weightsToBasisPoints([1, 1, 1])).toEqual([3334, 3333, 3333]);
	});

	it("keeps all-zero weights at 0 basis points", () => {
		expect(weightsToBasisPoints([0, 0, 0])).toEqual([0, 0, 0]);
	});

	it("rebalances unlocked rows after editing one row", () => {
		const result = rebalanceAfterEdit(
			[
				{ rowId: "u1", basisPoints: 5000, locked: false },
				{ rowId: "u2", basisPoints: 3000, locked: false },
				{ rowId: "u3", basisPoints: 2000, locked: false },
			],
			"u1",
			7000,
		);
		expect(result.ok).toBe(true);
		if (!result.ok) {
			return;
		}
		expect(result.rows.map((row) => row.basisPoints)).toEqual([
			7000, 1800, 1200,
		]);
		expect(result.totalBasisPoints).toBe(RATIO_BASIS_POINTS);
	});

	it("blocks over-capacity edits when locked rows cannot absorb", () => {
		const result = rebalanceAfterEdit(
			[
				{ rowId: "u1", basisPoints: 8000, locked: true },
				{ rowId: "u2", basisPoints: 2000, locked: true },
			],
			"u1",
			9000,
		);
		expect(result.ok).toBe(false);
		if (result.ok) {
			return;
		}
		expect(result.reason).toContain("exceeds 100%");
	});

	it("blocks save path when all rows are locked and total != 100%", () => {
		const result = rebalanceAfterEdit(
			[
				{ rowId: "u1", basisPoints: 5000, locked: true },
				{ rowId: "u2", basisPoints: 4000, locked: true },
			],
			"u1",
			5000,
		);
		expect(result.ok).toBe(false);
		if (result.ok) {
			return;
		}
		expect(result.reason).toContain("Unlock at least one row");
	});

	it("converts basis points to integer weights with largest remainder", () => {
		const weights = basisPointsToWeights([3333, 3333, 3334]);
		expect(weights.reduce((acc, value) => acc + value, 0)).toBe(10_000);
		expect(weights).toEqual([3333, 3333, 3334]);
	});

	it("parses percent input with range and decimal checks", () => {
		expect(parsePercentInput("12.34")).toEqual({
			ok: true,
			basisPoints: 1234,
		});
		expect(parsePercentInput("100.001").ok).toBe(false);
		expect(parsePercentInput("-1").ok).toBe(false);
	});
});
