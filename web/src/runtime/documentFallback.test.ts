import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { renderDocumentFallback } from "./documentFallback";

describe("document fallback", () => {
	beforeEach(() => {
		document.body.innerHTML = '<div id="root"></div>';
	});

	afterEach(() => {
		document.body.innerHTML = "";
	});

	it("renders a dependency-free reload action", () => {
		const reload = vi.fn();

		renderDocumentFallback(new Error("bootstrap failed"), {
			onReload: reload,
			buildId: "3.22.5-abc123",
		});

		expect(document.querySelector("#root h1")).toHaveTextContent(
			"xp could not start",
		);
		expect(
			document.querySelector("#root [data-action=reload]"),
		).toHaveTextContent("Reload app");

		(
			document.querySelector("[data-action=reload]") as HTMLButtonElement
		).click();
		expect(reload).toHaveBeenCalledTimes(1);
	});

	it("escapes failure details before inserting them into the document", () => {
		renderDocumentFallback(
			new Error(
				'<img src=x onerror="alert(1)"> Authorization: Bearer secret-token',
			),
			{
				onReload: vi.fn(),
				path: "/nodes?login_token=secret-login-token",
			},
		);

		expect(document.querySelector("#root img")).toBeNull();
		expect(document.querySelector("#root")?.innerHTML).toContain("&lt;img");
		expect(document.querySelector("#root")?.innerHTML).not.toContain(
			"secret-token",
		);
		expect(document.querySelector("#root")?.innerHTML).not.toContain(
			"secret-login-token",
		);
		expect(document.querySelector("#root")?.innerHTML).toContain(
			"path: /nodes",
		);
	});
});
