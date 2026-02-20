import { describe, expect, it } from "vitest";

import { computeEndpointProbeStatus } from "./endpointProbeStatus";

describe("computeEndpointProbeStatus", () => {
	it("treats incomplete buckets as missing", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 2,
				okCount: 0,
				skippedCount: 0,
			}),
		).toBe("missing");

		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 2,
				okCount: 1,
				skippedCount: 0,
			}),
		).toBe("missing");
	});

	it("returns down when all tested samples fail", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 0,
				skippedCount: 0,
			}),
		).toBe("down");
	});

	it("returns up when all tested samples are ok", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 3,
				skippedCount: 0,
			}),
		).toBe("up");
	});

	it("returns degraded when tested samples are mixed", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 2,
				skippedCount: 0,
			}),
		).toBe("degraded");
	});

	it("allows skipped samples when all tested samples are ok", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 2,
				skippedCount: 1,
			}),
		).toBe("up");
	});

	it("returns missing when all samples are skipped", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 1,
				sampleCount: 1,
				okCount: 0,
				skippedCount: 1,
			}),
		).toBe("missing");
	});

	it("returns degraded when tested samples are mixed even with skips", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 1,
				skippedCount: 1,
			}),
		).toBe("degraded");
	});

	it("returns down when all tested samples fail even with skips", () => {
		expect(
			computeEndpointProbeStatus({
				expectedNodes: 3,
				sampleCount: 3,
				okCount: 0,
				skippedCount: 1,
			}),
		).toBe("down");
	});
});
