import { describe, expect, it } from "vitest";

import { validateRealityServerName } from "./realityServerName";

describe("validateRealityServerName", () => {
	it("accepts typical hostnames used as SNI", () => {
		expect(validateRealityServerName("public.sn.files.1drv.com")).toBeNull();
		expect(validateRealityServerName("oneclient.sfx.ms")).toBeNull();
		expect(
			validateRealityServerName("  public.sn.files.1drv.com  "),
		).toBeNull();
	});

	it("rejects common copy/paste mistakes (url/path/port)", () => {
		expect(
			validateRealityServerName("https://public.sn.files.1drv.com"),
		).not.toBeNull();
		expect(
			validateRealityServerName("public.sn.files.1drv.com/path"),
		).not.toBeNull();
		expect(
			validateRealityServerName("public.sn.files.1drv.com:443"),
		).not.toBeNull();
	});

	it("rejects invalid hostname formats", () => {
		expect(validateRealityServerName("")).not.toBeNull();
		expect(validateRealityServerName("cc.c")).not.toBeNull();
		expect(validateRealityServerName("localhost")).not.toBeNull();
		expect(validateRealityServerName("a..b.com")).not.toBeNull();
		expect(validateRealityServerName(".example.com")).not.toBeNull();
		expect(validateRealityServerName("example.com.")).not.toBeNull();
		expect(validateRealityServerName("ex_ample.com")).not.toBeNull();
		expect(validateRealityServerName("-example.com")).not.toBeNull();
		expect(validateRealityServerName("example-.com")).not.toBeNull();
	});
});
