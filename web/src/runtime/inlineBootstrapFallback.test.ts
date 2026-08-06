import { describe, expect, it, vi } from "vitest";

import { inlineBootstrapFallback } from "./inlineBootstrapFallback";

describe("inline bootstrap fallback", () => {
	it("renders before the entry module is available", () => {
		document.body.innerHTML = '<div id="root"></div>';
		const reload = vi.fn();
		vi.stubGlobal("location", { reload });
		new Function(inlineBootstrapFallback("fixture-build"))();

		const script = document.createElement("script");
		document.head.append(script);
		script.dispatchEvent(new Event("error"));

		expect(
			document.querySelector("[data-xp-document-fallback]"),
		).not.toBeNull();
		expect(document.body.textContent).toContain("fixture-build");
		(
			document.querySelector("[data-action=reload]") as HTMLButtonElement
		).click();
		expect(reload).toHaveBeenCalledOnce();
	});

	it("renders when entry module evaluation fails after download", () => {
		document.body.innerHTML = '<div id="root"></div>';
		new Function(inlineBootstrapFallback("fixture-build"))();

		window.dispatchEvent(
			new ErrorEvent("error", {
				error: new Error("module evaluation failed"),
				message: "module evaluation failed",
			}),
		);

		expect(
			document.querySelector("[data-xp-document-fallback]"),
		).not.toBeNull();
	});

	it("escapes untrusted build IDs in the static diagnostic", () => {
		document.body.innerHTML = '<div id="root"></div>';
		const maliciousBuildId = '</script><img src=x onerror="alert(1)">';
		const fallback = inlineBootstrapFallback(maliciousBuildId);
		new Function(fallback)();

		const script = document.createElement("script");
		document.head.append(script);
		script.dispatchEvent(new Event("error"));

		expect(fallback).not.toContain("</script><img");
		expect(
			document.querySelector("[data-xp-document-fallback] pre"),
		).toHaveTextContent(maliciousBuildId);
		expect(document.querySelector("img")).toBeNull();
	});

	it("can be concatenated with the build declaration", () => {
		expect(() => {
			new Function(
				`${inlineBootstrapFallback("fixture-build")}window.__inlineLoaded=true;`,
			)();
		}).not.toThrow();
		expect(
			(window as Window & { __inlineLoaded?: boolean }).__inlineLoaded,
		).toBe(true);
	});

	it("does not replace a React app after startup", () => {
		document.body.innerHTML =
			'<div id="root" data-xp-react-ready="true">ready</div>';
		new Function(inlineBootstrapFallback("fixture-build"))();
		const script = document.createElement("script");
		document.head.append(script);

		script.dispatchEvent(new Event("error"));

		expect(document.getElementById("root")).toHaveTextContent("ready");
		expect(document.querySelector("[data-xp-document-fallback]")).toBeNull();
	});
});
