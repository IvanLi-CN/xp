import { describe, expect, it } from "vitest";

import {
	AdminServiceMonitorSchema,
	ServiceMonitorTargetSchema,
	monitorKind,
	monitorTargetLabel,
} from "./adminServiceMonitors";

describe("service monitor API schema", () => {
	it("parses the backend's internally tagged target contract", () => {
		const target = ServiceMonitorTargetSchema.parse({
			kind: "https",
			url: "https://status.example.com/health",
			method: "get",
			accepted_statuses: [{ start: 200, end: 399 }],
		});
		expect(monitorKind(target)).toBe("https");
		expect(monitorTargetLabel(target)).toBe(
			"https://status.example.com/health",
		);
	});

	it("rejects the obsolete externally tagged target shape", () => {
		expect(() =>
			AdminServiceMonitorSchema.parse({
				monitor_id: "01JMONITOR00000000000000001",
				name: "Public API health",
				target: { https: { url: "https://status.example.com" } },
				interval_seconds: 60,
				observer_policy: { mode: "exclude", node_ids: [] },
				lifecycle: "active",
				revision: 1,
				revision_effective_at_unix_seconds: 60,
			}),
		).toThrow();
	});
});
