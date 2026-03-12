import { describe, expect, it } from "vitest";

import { computeEndpointProbeStatus } from "./endpointProbeStatus";

describe("computeEndpointProbeStatus", () => {
	it("returns missing when participating nodes do not all report", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 3,
				sampleCount: 2,
				okCount: 1,
				skippedCount: 0,
			}),
		).toBe("missing");
	});

	it("ignores offline nodes that never participated", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 2,
				sampleCount: 2,
				okCount: 2,
				skippedCount: 0,
			}),
		).toBe("up");
	});

	it("returns down when all tested participant samples fail", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 2,
				sampleCount: 2,
				okCount: 0,
				skippedCount: 0,
			}),
		).toBe("down");
	});

	it("returns degraded when participant results are mixed", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 2,
				sampleCount: 2,
				okCount: 1,
				skippedCount: 0,
			}),
		).toBe("degraded");
	});

	it("allows skipped samples when all tested participant samples are ok", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 3,
				sampleCount: 3,
				okCount: 2,
				skippedCount: 1,
			}),
		).toBe("up");
	});

	it("returns missing when all samples are skipped", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 1,
				sampleCount: 1,
				okCount: 0,
				skippedCount: 1,
			}),
		).toBe("missing");
	});

	it("returns missing when no nodes participated", () => {
		expect(
			computeEndpointProbeStatus({
				participatingNodes: 0,
				sampleCount: 0,
				okCount: 0,
				skippedCount: 0,
			}),
		).toBe("missing");
	});
});
