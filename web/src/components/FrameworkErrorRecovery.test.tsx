import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BackendApiError } from "../api/backendError";
import type { CacheRecoveryResult } from "../runtime/frameworkErrorRecovery";
import { FrameworkErrorRecovery } from "./FrameworkErrorRecovery";

describe("FrameworkErrorRecovery", () => {
	const clipboardWriteText = vi.fn().mockResolvedValue(undefined);

	beforeEach(() => {
		clipboardWriteText.mockClear();
		Object.defineProperty(navigator, "clipboard", {
			configurable: true,
			value: { writeText: clipboardWriteText },
		});
	});

	it("shows classified cache guidance and recovery actions", async () => {
		const user = userEvent.setup();
		const onReload = vi.fn();
		const onClearCachedApp = vi
			.fn<() => Promise<CacheRecoveryResult>>()
			.mockResolvedValue({
				status: "skipped",
				reason: "replacement-unavailable",
				deleted: [],
			});

		render(
			<FrameworkErrorRecovery
				error={new Error("asset cache version mismatch")}
				category="cache-mismatch"
				onReload={onReload}
				onClearCachedApp={onClearCachedApp}
			/>,
		);

		expect(
			screen.getByRole("heading", {
				name: "The cached app assets are out of sync",
			}),
		).toBeInTheDocument();
		expect(screen.getByTestId("framework-error-category")).toHaveAttribute(
			"data-error-category",
			"cache-mismatch",
		);
		expect(
			screen.getByRole("button", { name: "Clear cached app and reload" }),
		).toBeInTheDocument();

		await user.click(screen.getByRole("button", { name: "Reload app" }));
		expect(onReload).toHaveBeenCalledTimes(1);
		await user.click(
			screen.getByRole("button", { name: "Clear cached app and reload" }),
		);
		expect(onClearCachedApp).toHaveBeenCalledTimes(1);
		await screen.findByText(
			"The current app cache was left untouched because a complete replacement was not available.",
		);
		expect(onReload).toHaveBeenCalledTimes(1);
	});

	it("keeps cache recovery hidden for an offline failure", () => {
		render(
			<FrameworkErrorRecovery
				error={new TypeError("Failed to fetch")}
				category="offline"
				isOnline={false}
				onReload={vi.fn()}
			/>,
		);

		expect(
			screen.getByRole("heading", { name: "You are offline" }),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Clear cached app and reload" }),
		).not.toBeInTheDocument();
	});

	it("shows stronger guidance after a repeated unknown failure and copies diagnostics", async () => {
		render(
			<FrameworkErrorRecovery
				error={new Error("unexpected runtime failure")}
				category="unknown"
				repeatFailure
				onClearCachedApp={vi.fn().mockResolvedValue({
					status: "cleared",
					deleted: ["xp-app-shell-build-1"],
				})}
			/>,
		);

		expect(screen.getByTestId("repeat-failure")).toHaveTextContent(
			"This happened again. Safe cache recovery is available.",
		);
		fireEvent.click(screen.getByText("Technical details"));
		fireEvent.click(
			screen.getByRole("button", { name: "Copy diagnostic details" }),
		);
		await vi.waitFor(() => {
			expect(clipboardWriteText).toHaveBeenCalledWith(
				expect.stringContaining("category: unknown"),
			);
		});
	});

	it("classifies API incompatibility and keeps diagnostics available", () => {
		render(
			<FrameworkErrorRecovery
				error={
					new BackendApiError({
						status: 409,
						code: "api_incompatible",
						message: "internal compatibility details",
					})
				}
			/>,
		);

		expect(
			screen.getByRole("heading", {
				name: "The backend does not support this web app",
			}),
		).toBeInTheDocument();
		expect(screen.getByTestId("framework-error-diagnostics")).toHaveTextContent(
			"category: api-incompatibility",
		);
	});
});
