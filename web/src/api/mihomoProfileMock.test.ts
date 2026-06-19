import { describe, expect, it } from "vitest";

import {
	normalizeMockMihomoProfilePayload,
	normalizeMockStoredMihomoProfile,
} from "../../tests/e2e/helpers";

describe("normalizeMockMihomoProfilePayload", () => {
	it("keeps top-level proxies and proxy-providers in mixin_yaml", () => {
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
		expect(result.profile.mixin_yaml).toContain("proxies:");
		expect(result.profile.mixin_yaml).toContain("proxy-providers:");
		expect(result.profile.extra_proxies_yaml).toBe("");
		expect(result.profile.extra_proxy_providers_yaml).toBe("");
	});

	it("rejects empty mixin and legacy template fields", () => {
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
	});
});

describe("normalizeMockStoredMihomoProfile", () => {
	it("returns stored profile raw for admin GET", () => {
		expect(
			normalizeMockStoredMihomoProfile({
				mixin_yaml: `port: 0
proxies:
  - name: inline-a
    type: ss
    server: inline-a.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: a:def
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a
rules: []
`,
				extra_proxies_yaml: `- name: existing-extra
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: extra:def
  udp: true
`,
				extra_proxy_providers_yaml: `providerB:
  type: http
  path: ./provider-b.yaml
  url: https://example.com/sub-b
`,
			}),
		).toEqual({
			mixin_yaml: `port: 0
proxies:
  - name: inline-a
    type: ss
    server: inline-a.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: a:def
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a
rules: []
`,
			extra_proxies_yaml: `- name: existing-extra
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: extra:def
  udp: true
`,
			extra_proxy_providers_yaml: `providerB:
  type: http
  path: ./provider-b.yaml
  url: https://example.com/sub-b
`,
		});
	});

	it("falls back to raw stored text when mixin_yaml is invalid", () => {
		expect(
			normalizeMockStoredMihomoProfile({
				mixin_yaml: `port: [
`,
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			}),
		).toEqual({
			mixin_yaml: `port: [
`,
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		});
	});
});
