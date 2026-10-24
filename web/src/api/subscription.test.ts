import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	SUBSCRIPTION_FORMAT_OPTIONS,
	buildSubscriptionUrl,
} from "./subscription";

describe("subscription format contract", () => {
	it("keeps UI-selectable formats aligned with the backend contract", () => {
		expect(DEFAULT_SUBSCRIPTION_FORMAT).toBe("raw");
		expect(SUBSCRIPTION_FORMAT_OPTIONS).toEqual([
			{ value: "raw", label: "Raw" },
			{ value: "clash", label: "Clash" },
			{ value: "mihomo", label: "Mihomo" },
		]);
		expect(
			SUBSCRIPTION_FORMAT_OPTIONS.some((option) =>
				/default|legacy/i.test(`${option.value} ${option.label}`),
			),
		).toBe(false);
	});

	it("builds external URLs from the active backend origin", () => {
		expect(
			buildSubscriptionUrl({
				origin: "https://node.example",
				subscriptionToken: fixtureCatalog.token.fixture254(),
				format: "mihomo",
				externalResources: "mirror",
			}),
		).toBe(
			"https://node.example/api/sub/fixture-token-254?format=mihomo&external_resources=mirror",
		);
	});

	it("keeps server-side URLs relative when no origin is available", () => {
		expect(
			buildSubscriptionUrl({
				subscriptionToken: fixtureCatalog.token.fixture257(),
				format: "raw",
			}),
		).toBe("/api/sub/fixture-token-257?format=raw");
	});
});
