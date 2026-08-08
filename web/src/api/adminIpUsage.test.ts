import { describe, expect, it } from "vitest";

import { fixtureStoryData } from "../fixture-policy/storybook";
import {
	AdminIpGeoSourceSchema,
	AdminNodeIpUsageResponseSchema,
	AdminUserIpUsageResponseSchema,
} from "./adminIpUsage";

describe("admin IP usage schemas", () => {
	it("accepts node IP usage payload", () => {
		const parsed = AdminNodeIpUsageResponseSchema.parse(
			fixtureStoryData.nodeIpUsage()["24h"],
		);

		expect(parsed.timeline[0]?.segments).toHaveLength(1);
	});

	it("accepts grouped user IP usage payload", () => {
		const parsed = AdminUserIpUsageResponseSchema.parse(
			fixtureStoryData.partialUserIpUsage()["7d"],
		);

		expect(parsed.partial).toBe(true);
		expect(parsed.unreachable_nodes).toHaveLength(1);
	});

	it("accepts legacy geo_source values for rolling upgrades", () => {
		expect(AdminIpGeoSourceSchema.parse("managed_dbip_lite")).toBe(
			"managed_dbip_lite",
		);
		expect(AdminIpGeoSourceSchema.parse("external_override")).toBe(
			"external_override",
		);
		expect(AdminIpGeoSourceSchema.parse("missing")).toBe("missing");
	});
});
