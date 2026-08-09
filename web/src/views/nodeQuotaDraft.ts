import type { AdminNode } from "../api/adminNodes";
import type { NodeQuotaReset } from "../api/quotaReset";

export type NodeQuotaDraft = {
	resetPolicy: "monthly" | "unlimited";
	resetDay: unknown;
	resetTzOffsetMinutes: string;
};

export function nodeQuotaDraftFromNode(node: AdminNode): NodeQuotaDraft {
	const reset = node.quota_reset;
	return {
		resetPolicy: reset.policy === "monthly" ? "monthly" : "unlimited",
		resetDay: reset.policy === "monthly" ? reset.day_of_month : 1,
		resetTzOffsetMinutes:
			reset.tz_offset_minutes === null || reset.tz_offset_minutes === undefined
				? ""
				: String(reset.tz_offset_minutes),
	};
}

export function toNodeQuotaReset(draft: NodeQuotaDraft): NodeQuotaReset {
	const offset = draft.resetTzOffsetMinutes.trim();
	const tzOffset = offset === "" ? undefined : Number(offset);
	return draft.resetPolicy === "monthly"
		? {
				policy: "monthly",
				day_of_month: Number(draft.resetDay),
				...(tzOffset === undefined ? {} : { tz_offset_minutes: tzOffset }),
			}
		: {
				policy: "unlimited",
				...(tzOffset === undefined ? {} : { tz_offset_minutes: tzOffset }),
			};
}

export function isNodeQuotaDraftDirty(
	node: AdminNode | undefined,
	draft: NodeQuotaDraft,
): boolean {
	if (!node) return false;
	const reset = node.quota_reset;
	const day = reset.policy === "monthly" ? reset.day_of_month : 1;
	const offset =
		reset.tz_offset_minutes === null || reset.tz_offset_minutes === undefined
			? ""
			: String(reset.tz_offset_minutes);
	return (
		draft.resetPolicy !== reset.policy ||
		(draft.resetPolicy === "monthly" && Number(draft.resetDay) !== day) ||
		draft.resetTzOffsetMinutes.trim() !== offset
	);
}
