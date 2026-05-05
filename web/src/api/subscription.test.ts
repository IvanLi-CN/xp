import { describe, expect, it } from "vitest";

import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	SUBSCRIPTION_FORMAT_OPTIONS,
} from "./subscription";

describe("subscription format contract", () => {
	it("keeps UI-selectable formats aligned with the backend contract", () => {
		expect(DEFAULT_SUBSCRIPTION_FORMAT).toBe("raw");
		expect(SUBSCRIPTION_FORMAT_OPTIONS).toEqual([
			{ value: "raw", label: "raw" },
			{ value: "clash", label: "clash" },
			{ value: "mihomo", label: "mihomo(provider)" },
		]);
		expect(
			SUBSCRIPTION_FORMAT_OPTIONS.some((option) =>
				/default|legacy/i.test(`${option.value} ${option.label}`),
			),
		).toBe(false);
	});
});
