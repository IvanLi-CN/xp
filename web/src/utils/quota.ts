const MIB_BYTES = 1024 * 1024;
const GIB_BYTES = 1024 * 1024 * 1024;

export type QuotaParseResult =
	| { ok: true; bytes: number }
	| { ok: false; error: string };

function normalizeUnit(unitRaw: string): string {
	return unitRaw.trim().toLowerCase().replaceAll(/\s+/g, "");
}

function parseUnitFactor(
	unitRaw: string,
): { ok: true; factor: number; unit: "MiB" | "GiB" } | { ok: false } {
	const unit = normalizeUnit(unitRaw);
	if (!unit) return { ok: true, factor: MIB_BYTES, unit: "MiB" };

	const mib = new Set([
		"m",
		"mi",
		"mib",
		"mibyte",
		"mibytes",
		"mebibyte",
		"mebibytes",
		"mb", // compatibility: treat as MiB
		"mbyte",
		"mbytes",
	]);
	if (mib.has(unit)) return { ok: true, factor: MIB_BYTES, unit: "MiB" };

	const gib = new Set([
		"g",
		"gi",
		"gib",
		"gibyte",
		"gibytes",
		"gibibyte",
		"gibibytes",
		"gb", // compatibility: treat as GiB
		"gbyte",
		"gbytes",
	]);
	if (gib.has(unit)) return { ok: true, factor: GIB_BYTES, unit: "GiB" };

	return { ok: false };
}

export function parseQuotaInputToBytes(input: string): QuotaParseResult {
	const trimmed = input.trim();
	if (!trimmed) return { ok: false, error: "Quota is required." };

	const match = trimmed.match(/^([+-]?\d+(?:\.\d+)?)\s*([a-zA-Z ]*)$/);
	if (!match) return { ok: false, error: "Invalid quota format." };

	const rawNumber = match[1] ?? "";
	const rawUnit = match[2] ?? "";

	const value = Number(rawNumber);
	if (!Number.isFinite(value)) return { ok: false, error: "Invalid number." };
	if (value < 0) return { ok: false, error: "Quota must be zero or greater." };

	const unit = parseUnitFactor(rawUnit);
	if (!unit.ok) {
		return {
			ok: false,
			error: "Unsupported unit (use MiB/GiB; also accepts M/G, MB/GB).",
		};
	}

	const bytes = Math.round(value * unit.factor);
	if (!Number.isSafeInteger(bytes)) {
		return {
			ok: false,
			error: `Quota is too large (must be <= ${Number.MAX_SAFE_INTEGER}).`,
		};
	}
	if (bytes < 0) return { ok: false, error: "Quota must be zero or greater." };

	return { ok: true, bytes };
}

function formatRounded(value: number, fractionDigits: number): string {
	const pow = 10 ** fractionDigits;
	const rounded = Math.round(value * pow) / pow;
	return rounded.toFixed(fractionDigits).replace(/\.?0+$/, "");
}

export function formatQuotaBytesHuman(bytes: number): string {
	if (bytes === 0) return "0";
	if (bytes >= GIB_BYTES) {
		return `${formatRounded(bytes / GIB_BYTES, 2)} GiB`;
	}
	return `${formatRounded(bytes / MIB_BYTES, 2)} MiB`;
}

export function formatQuotaBytesCompactInput(bytes: number): string {
	if (bytes === 0) return "0";
	if (bytes % GIB_BYTES === 0) return `${bytes / GIB_BYTES}GiB`;
	if (bytes % MIB_BYTES === 0) return `${bytes / MIB_BYTES}MiB`;
	if (bytes >= GIB_BYTES) return `${formatRounded(bytes / GIB_BYTES, 2)}GiB`;
	return `${formatRounded(bytes / MIB_BYTES, 2)}MiB`;
}
