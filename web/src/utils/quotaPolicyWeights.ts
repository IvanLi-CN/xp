export const RATIO_BASIS_POINTS = 10_000;
export const WEIGHT_DISCRETIZATION_BASE = 10_000;

export type RatioEditableRow = {
	rowId: string;
	basisPoints: number;
	locked: boolean;
};

export type RebalanceResult =
	| {
			ok: true;
			rows: RatioEditableRow[];
			totalBasisPoints: number;
	  }
	| {
			ok: false;
			reason: string;
			rows: RatioEditableRow[];
			totalBasisPoints: number;
	  };

function clampInt(value: number, min: number, max: number): number {
	return Math.min(max, Math.max(min, Math.round(value)));
}

function sum(values: number[]): number {
	return values.reduce((acc, value) => acc + value, 0);
}

function distributeByWeights(total: number, weights: number[]): number[] {
	if (weights.length === 0) {
		return [];
	}
	if (total <= 0) {
		return new Array(weights.length).fill(0);
	}

	const safeWeights = weights.map((weight) => Math.max(0, weight));
	const weightSum = sum(safeWeights);
	const normalizedWeights =
		weightSum > 0 ? safeWeights : new Array(weights.length).fill(1);
	const normalizedSum = sum(normalizedWeights);

	const allocations = normalizedWeights.map((weight, index) => {
		const exact = (weight / normalizedSum) * total;
		const floor = Math.floor(exact);
		return {
			index,
			floor,
			remainder: exact - floor,
		};
	});

	let remaining =
		total - allocations.reduce((acc, item) => acc + item.floor, 0);
	allocations.sort(
		(a, b) =>
			b.remainder - a.remainder ||
			normalizedWeights[b.index] - normalizedWeights[a.index] ||
			a.index - b.index,
	);
	for (let i = 0; i < allocations.length && remaining > 0; i += 1) {
		const allocation = allocations[i];
		if (!allocation) {
			continue;
		}
		allocation.floor += 1;
		remaining -= 1;
	}

	const out = new Array(weights.length).fill(0);
	for (const item of allocations) {
		out[item.index] = item.floor;
	}
	return out;
}

export function formatPercentFromBasisPoints(basisPoints: number): string {
	return (basisPoints / 100).toFixed(2);
}

export function parsePercentInput(
	raw: string,
): { ok: true; basisPoints: number } | { ok: false; error: string } {
	const trimmed = raw.trim();
	if (trimmed.length === 0) {
		return { ok: true, basisPoints: 0 };
	}
	if (!/^\d{1,3}(?:\.\d{0,2})?$/.test(trimmed)) {
		return {
			ok: false,
			error: "Percentage must be a number with up to 2 decimals.",
		};
	}
	const value = Number(trimmed);
	if (!Number.isFinite(value)) {
		return { ok: false, error: "Percentage must be a finite number." };
	}
	if (value < 0 || value > 100) {
		return { ok: false, error: "Percentage must be between 0 and 100." };
	}
	return {
		ok: true,
		basisPoints: clampInt(value * 100, 0, RATIO_BASIS_POINTS),
	};
}

export function weightsToBasisPoints(weights: number[]): number[] {
	if (weights.length === 0) {
		return [];
	}
	const safeWeights = weights.map((weight) => Math.max(0, Math.floor(weight)));
	const total = sum(safeWeights);
	if (total <= 0) {
		return new Array(weights.length).fill(0);
	}
	return distributeByWeights(RATIO_BASIS_POINTS, safeWeights);
}

export function basisPointsToWeights(
	basisPoints: number[],
	base: number = WEIGHT_DISCRETIZATION_BASE,
): number[] {
	if (basisPoints.length === 0) {
		return [];
	}
	if (base <= 0) {
		throw new Error("base must be positive");
	}
	const safeBasis = basisPoints.map((value) => Math.max(0, Math.floor(value)));
	const total = sum(safeBasis);
	if (total <= 0) {
		return new Array(basisPoints.length).fill(0);
	}
	return distributeByWeights(base, safeBasis);
}

function totalBasisPoints(rows: RatioEditableRow[]): number {
	return rows.reduce((acc, row) => acc + row.basisPoints, 0);
}

export function rebalanceAfterEdit(
	rows: RatioEditableRow[],
	editedRowId: string,
	nextBasisPointsRaw: number,
): RebalanceResult {
	const editedIndex = rows.findIndex((row) => row.rowId === editedRowId);
	if (editedIndex < 0) {
		return {
			ok: false,
			reason: "Edited row is missing.",
			rows,
			totalBasisPoints: totalBasisPoints(rows),
		};
	}

	const nextBasisPoints = clampInt(nextBasisPointsRaw, 0, RATIO_BASIS_POINTS);
	const nextRows = rows.map((row, index) => ({
		...row,
		basisPoints: index === editedIndex ? nextBasisPoints : row.basisPoints,
	}));

	const adjustableIndexes: number[] = [];
	let fixedTotal = 0;
	for (let index = 0; index < nextRows.length; index += 1) {
		const row = nextRows[index];
		if (!row) {
			continue;
		}
		if (index === editedIndex) {
			fixedTotal += row.basisPoints;
			continue;
		}
		if (row.locked) {
			fixedTotal += row.basisPoints;
			continue;
		}
		adjustableIndexes.push(index);
	}

	const remaining = RATIO_BASIS_POINTS - fixedTotal;
	if (remaining < 0) {
		return {
			ok: false,
			reason: "Total exceeds 100%. Unlock rows or lower the edited value.",
			rows: nextRows,
			totalBasisPoints: totalBasisPoints(nextRows),
		};
	}

	if (adjustableIndexes.length === 0) {
		const total = totalBasisPoints(nextRows);
		if (total !== RATIO_BASIS_POINTS) {
			return {
				ok: false,
				reason:
					"All rows are locked. Unlock at least one row to keep total at 100%.",
				rows: nextRows,
				totalBasisPoints: total,
			};
		}
		return {
			ok: true,
			rows: nextRows,
			totalBasisPoints: total,
		};
	}

	const sourceWeights = adjustableIndexes.map((index) => {
		const row = rows[index];
		return Math.max(0, row ? row.basisPoints : 0);
	});
	const allocations = distributeByWeights(remaining, sourceWeights);
	for (let i = 0; i < adjustableIndexes.length; i += 1) {
		const rowIndex = adjustableIndexes[i];
		const nextRow = rowIndex === undefined ? undefined : nextRows[rowIndex];
		const allocation = allocations[i];
		if (!nextRow || allocation === undefined) {
			continue;
		}
		nextRow.basisPoints = allocation;
	}

	return {
		ok: true,
		rows: nextRows,
		totalBasisPoints: totalBasisPoints(nextRows),
	};
}
