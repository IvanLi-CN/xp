import { describe, expect, it } from "vitest";

import { AdminConfigResponseSchema } from "./adminConfig";

const baseConfig = {
	bind: "127.0.0.1:62416",
	xray_api_addr: "127.0.0.1:10085",
	data_dir: "/var/lib/xp",
	node_name: "fixture-node",
	access_host: "fixture.example",
	api_base_url: "https://fixture.example",
	vless_https_canary_bind: "127.0.0.1:18080",
	quota_poll_interval_secs: 60,
	quota_auto_unban: false,
	ip_geo_enabled: false,
	ip_geo_origin: "",
	admin_token_present: true,
	admin_token_masked: "****",
};

describe("admin config schemas", () => {
	it("accepts legacy responses without the current-only resource policy", () => {
		const parsed = AdminConfigResponseSchema.parse(baseConfig);

		expect(parsed.mihomo_resource_allow_private_targets).toBeUndefined();
	});

	it("preserves the current resource policy when advertised", () => {
		const parsed = AdminConfigResponseSchema.parse({
			...baseConfig,
			mihomo_resource_allow_private_targets: false,
		});

		expect(parsed.mihomo_resource_allow_private_targets).toBe(false);
	});
});
