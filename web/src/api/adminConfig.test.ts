import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { AdminConfigResponseSchema } from "./adminConfig";

const baseConfig = {
	bind: fixtureCatalog.address.loopback39043(),
	xray_api_addr: fixtureCatalog.address.loopback49043(),
	data_dir: "/var/lib/xp",
	node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	access_host: fixtureCatalog.host.primary(),
	api_base_url: fixtureCatalog.url.primaryApi(),
	vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
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
