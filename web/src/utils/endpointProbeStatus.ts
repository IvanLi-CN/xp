import type { EndpointProbeStatus } from "../api/adminEndpoints";

export function computeEndpointProbeStatus(args: {
	expectedNodes: number;
	sampleCount: number;
	okCount: number;
	skippedCount: number;
}): EndpointProbeStatus {
	if (args.expectedNodes === 0) return "missing";
	if (args.sampleCount === 0) return "missing";
	if (args.sampleCount < args.expectedNodes) return "missing";

	const testedCount = Math.max(0, args.sampleCount - args.skippedCount);
	if (testedCount === 0) return "missing";

	if (args.okCount === 0) return "down";
	if (args.okCount >= testedCount) return "up";
	return "degraded";
}
