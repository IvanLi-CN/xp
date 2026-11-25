import { describe, expect, it } from "vitest";

import { peerConnectivityLabels } from "./systemStatusLabels";

describe("peerConnectivityLabels", () => {
	it("keeps direct unavailable separate from public fallback available", () => {
		expect(
			peerConnectivityLabels({
				direct_connectivity: "unavailable",
				public_fallback_health: "available",
			}),
		).toEqual({
			direct: "Direct unavailable",
			public: "Public fallback available",
		});
	});

	it("renders both unknown facts without inventing a healthy path", () => {
		expect(
			peerConnectivityLabels({
				direct_connectivity: "unknown",
				public_fallback_health: "unknown",
			}),
		).toEqual({
			direct: "Direct unknown",
			public: "Public fallback unknown",
		});
	});
});
