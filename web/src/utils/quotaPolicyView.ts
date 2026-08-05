import { RATIO_BASIS_POINTS } from "./quotaPolicyWeights";

export function formatUtcOffsetMinutes(minutes: number): string {
	const sign = minutes >= 0 ? "+" : "-";
	const abs = Math.abs(minutes);
	const hh = String(Math.floor(abs / 60)).padStart(2, "0");
	const mm = String(abs % 60).padStart(2, "0");
	return `UTC${sign}${hh}:${mm}`;
}

export function formatNodeQuotaResetBrief(q: {
	policy: "monthly" | "unlimited";
	day_of_month?: number;
	tz_offset_minutes?: number | null;
}): string {
	const tz =
		q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
			? "(local)"
			: formatUtcOffsetMinutes(q.tz_offset_minutes);
	return q.policy === "monthly"
		? `monthly@${q.day_of_month ?? 1} ${tz}`
		: `unlimited ${tz}`;
}

export function ratioStatusTone(
	totalBasisPoints: number,
): "success" | "warning" | "error" {
	if (totalBasisPoints === RATIO_BASIS_POINTS) return "success";
	if (totalBasisPoints < RATIO_BASIS_POINTS) return "warning";
	return "error";
}
