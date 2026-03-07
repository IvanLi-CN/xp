import { describe, expect, it } from "vitest";

import {
	normalizeMockMihomoProfilePayload,
	normalizeMockStoredMihomoProfile,
} from "../../tests/e2e/helpers";

describe("normalizeMockMihomoProfilePayload", () => {
	it("autosplits top-level proxies and proxy-providers", () => {
		const result = normalizeMockMihomoProfilePayload({
			mixin_yaml: `port: 0
proxies:
  - name: custom-direct
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: abc:def
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a.yaml
    url: https://example.com/sub-a
rules: []
`,
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		});

		expect(result.ok).toBe(true);
		if (!result.ok) {
			throw new Error(result.message);
		}
		expect(result.profile.mixin_yaml).not.toContain("proxies:");
		expect(result.profile.mixin_yaml).not.toContain("proxy-providers:");
		expect(result.profile.extra_proxies_yaml).toContain("custom-direct");
		expect(result.profile.extra_proxy_providers_yaml).toContain("providerA");
	});

	it("rejects empty mixin, legacy template fields, and conflicting extracted sections", () => {
		expect(
			normalizeMockMihomoProfilePayload({
				mixin_yaml: "",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			}),
		).toEqual({ ok: false, message: "mixin_yaml is required" });

		expect(
			normalizeMockMihomoProfilePayload({
				template_yaml: "port: 0\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			}),
		).toEqual({ ok: false, message: "template_yaml is no longer supported" });

		expect(
			normalizeMockMihomoProfilePayload({
				mixin_yaml: `port: 0
proxies: []
`,
				extra_proxies_yaml: "- name: duplicate\n",
				extra_proxy_providers_yaml: "",
			}),
		).toEqual({
			ok: false,
			message: "mixin_yaml.proxies cannot be combined with extra_proxies_yaml",
		});
	});
});

describe("normalizeMockStoredMihomoProfile", () => {
	it("falls back to raw stored text when normalization fails", () => {
		expect(
			normalizeMockStoredMihomoProfile({
				mixin_yaml: "port: [\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			}),
		).toEqual({
			mixin_yaml: "port: [\n",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		});
	});
});
