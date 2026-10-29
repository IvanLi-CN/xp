import { z } from "zod";

export const MihomoXmuxSchema = z.object({
	max_connections: z.string(),
	max_concurrency: z.string(),
	c_max_reuse_times: z.string(),
	h_max_reusable_secs: z.string(),
	h_keep_alive_period: z.number().int().min(-1).max(2147483647),
});
export type MihomoXmuxConfig = z.infer<typeof MihomoXmuxSchema>;
export type MihomoXmuxDraft = {
	strategy: "connections" | "concurrency";
	maxConnections: string;
	maxConcurrency: string;
	reuseTimes: string;
	reusableSecs: string;
	keepAlive: string;
};
export const DEFAULT_XMUX_DRAFT: MihomoXmuxDraft = {
	strategy: "connections",
	maxConnections: "1",
	maxConcurrency: "16-32",
	reuseTimes: "0",
	reusableSecs: "0",
	keepAlive: "-1",
};

function validRange(value: string, positive: boolean): boolean {
	if (!/^(?:0|[1-9]\d*)(?:-(?:0|[1-9]\d*))?$/.test(value)) return false;
	const values = value.split("-").map(Number);
	const first = values[0];
	const last = values[1] ?? first;
	return (
		first <= last &&
		last <= 2147483647 &&
		(!positive || first > 0) &&
		(first !== 0 || value === "0")
	);
}

export function xmuxFromDraft(draft: MihomoXmuxDraft): MihomoXmuxConfig {
	const limit =
		draft.strategy === "connections"
			? draft.maxConnections
			: draft.maxConcurrency;
	if (
		!validRange(limit, true) ||
		!validRange(draft.reuseTimes, false) ||
		!validRange(draft.reusableSecs, false)
	) {
		throw new Error("Enter valid XMUX values: a number or an ascending range.");
	}
	if (!/^-?\d+$/.test(draft.keepAlive))
		throw new Error("Enter a valid keepalive period in seconds.");
	const keepAlive = Number(draft.keepAlive);
	if (
		!Number.isSafeInteger(keepAlive) ||
		keepAlive < -1 ||
		keepAlive > 2147483647
	)
		throw new Error("Keepalive must be -1 or greater.");
	return {
		max_connections: draft.strategy === "connections" ? limit : "0",
		max_concurrency: draft.strategy === "concurrency" ? limit : "0",
		c_max_reuse_times: draft.reuseTimes,
		h_max_reusable_secs: draft.reusableSecs,
		h_keep_alive_period: keepAlive,
	};
}

export function xmuxDraftFromValue(value: unknown): MihomoXmuxDraft {
	const parsed = MihomoXmuxSchema.safeParse(value);
	if (!parsed.success) return { ...DEFAULT_XMUX_DRAFT };
	const config = parsed.data;
	return {
		strategy: config.max_connections === "0" ? "concurrency" : "connections",
		maxConnections:
			config.max_connections === "0" ? "1" : config.max_connections,
		maxConcurrency:
			config.max_concurrency === "0" ? "16-32" : config.max_concurrency,
		reuseTimes: config.c_max_reuse_times,
		reusableSecs: config.h_max_reusable_secs,
		keepAlive: String(config.h_keep_alive_period),
	};
}
