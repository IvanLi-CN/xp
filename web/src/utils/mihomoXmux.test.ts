import { describe, expect, it } from "vitest";
import {
	DEFAULT_XMUX_DRAFT,
	xmuxDraftFromValue,
	xmuxFromDraft,
} from "./mihomoXmux";

describe("XMUX draft", () => {
	it("keeps existing defaults and produces an exclusive concurrency policy", () => {
		expect(xmuxFromDraft(DEFAULT_XMUX_DRAFT)).toEqual({
			max_connections: "1",
			max_concurrency: "0",
			c_max_reuse_times: "0",
			h_max_reusable_secs: "0",
			h_keep_alive_period: -1,
		});
		const draft = {
			...DEFAULT_XMUX_DRAFT,
			strategy: "concurrency" as const,
			reuseTimes: "20-40",
			keepAlive: "45",
		};
		const config = xmuxFromDraft(draft);
		expect(config.max_connections).toBe("0");
		expect(config.max_concurrency).toBe("16-32");
		expect(xmuxDraftFromValue(config)).toEqual(draft);
	});
	it("rejects malformed ranges and invalid keepalive", () => {
		for (const value of ["", "-1", "8-2", "0-1", "01", "2147483648"]) {
			expect(() =>
				xmuxFromDraft({ ...DEFAULT_XMUX_DRAFT, maxConnections: value }),
			).toThrow();
		}
		expect(() =>
			xmuxFromDraft({ ...DEFAULT_XMUX_DRAFT, keepAlive: "-2" }),
		).toThrow();
	});
});
