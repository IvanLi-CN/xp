import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { AdminUpgradeStatusResponseSchema } from "./adminUpgrade";

describe("AdminUpgradeStatusResponseSchema", () => {
	it("parses supported idle status", () => {
		const parsed = AdminUpgradeStatusResponseSchema.parse({
			support: {
				supported: true,
				reason: null,
				trigger: "systemd",
			},
			status: {
				state: "idle",
				target_tag: null,
				repo: null,
				started_at: null,
				finished_at: null,
				exit_code: null,
				message: null,
				updated_at: fixtureCatalog.timestamp.baseline(),
			},
		});

		expect(parsed.support.supported).toBe(true);
		expect(parsed.status.state).toBe("idle");
	});

	it("parses additive storage prediction", () => {
		const parsed = AdminUpgradeStatusResponseSchema.parse({
			support: {
				supported: true,
				storage: {
					install: {
						path: "/usr/local/bin",
						available_bytes: 100,
						reclaimable_bytes: 30,
						required_bytes: 128,
						sufficient_after_cleanup: true,
					},
					workspace: {
						path: "/tmp/xp-ops",
						available_bytes: 200,
						reclaimable_bytes: 0,
						required_bytes: 128,
						sufficient_after_cleanup: true,
					},
					cleanup_required: true,
				},
			},
			status: {
				state: "idle",
				updated_at: "2026-07-04T00:00:00Z",
			},
		});

		expect(parsed.support.storage?.cleanup_required).toBe(true);
	});

	it("parses unsupported container status", () => {
		const parsed = AdminUpgradeStatusResponseSchema.parse({
			support: {
				supported: false,
				reason: "container runtime",
				trigger: null,
			},
			status: {
				state: "unsupported",
				target_tag: "v0.2.0",
				repo: "IvanLi-CN/xp",
				started_at: null,
				finished_at: null,
				exit_code: null,
				message: "Use host-side image or Compose upgrade.",
				updated_at: fixtureCatalog.timestamp.baseline(),
			},
		});

		expect(parsed.support.reason).toBe("container runtime");
		expect(parsed.status.state).toBe("unsupported");
	});
});
