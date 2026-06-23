import { describe, expect, it, vi } from "vitest";

import {
	buildNodePageHref,
	buildRedirectTarget,
	resolveLoginRedirectFromHref,
	sanitizeRedirectPath,
	stripLoginToken,
} from "./navigation";

describe("navigation helpers", () => {
	it("sanitizes redirect paths to same-origin relative targets", () => {
		expect(sanitizeRedirectPath("/nodes?tab=runtime#history")).toBe(
			"/nodes?tab=runtime#history",
		);
		expect(sanitizeRedirectPath("https://example.com")).toBe("/");
		expect(sanitizeRedirectPath("//evil.example.com")).toBe("/");
		expect(sanitizeRedirectPath(undefined)).toBe("/");
	});

	it("builds redirect targets without login_token", () => {
		expect(
			buildRedirectTarget(
				"/nodes",
				"?view=grid&login_token=old-token",
				"#runtime",
			),
		).toBe("/nodes?view=grid#runtime");
	});

	it("resolves redirect targets from absolute href values", () => {
		vi.stubGlobal("window", {
			location: {
				origin: "https://node-a.example.com",
			},
		});

		expect(
			resolveLoginRedirectFromHref(
				"https://node-b.example.com/dashboard?foo=1&login_token=old#frag",
			),
		).toBe("/dashboard?foo=1#frag");
	});

	it("replaces login_token when building cross-node hrefs", () => {
		const resolved = buildNodePageHref(
			"https://node-b.example.com",
			"https://node-a.example.com/nodes?view=table&login_token=old-token#history",
			"fresh-token",
		);

		expect(resolved.disabled).toBe(false);
		if (resolved.disabled) return;
		expect(resolved.href).toBe(
			"https://node-b.example.com/nodes?view=table&login_token=fresh-token#history",
		);
	});

	it("disables cross-node hrefs for invalid base URLs", () => {
		expect(buildNodePageHref("http://node-b.example.com").disabled).toBe(true);
		expect(buildNodePageHref("")).toEqual({
			disabled: true,
			reason: "Open on node unavailable: API base URL is empty.",
		});
	});

	it("strips login_token from search params", () => {
		expect(
			stripLoginToken(
				new URLSearchParams("foo=1&login_token=old&bar=2"),
			).toString(),
		).toBe("foo=1&bar=2");
	});
});
