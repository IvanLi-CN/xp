import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	DocumentFallbackBoundary,
	FrameworkErrorBoundary,
	installDocumentFallbackHandlers,
} from "./FrameworkErrorBoundary";

function ThrowOnRender({ message = "render failed" }: { message?: string }) {
	const error = new Error(message);
	if (error instanceof Error) throw error;
	return null;
}

describe("framework error boundaries", () => {
	let consoleError: ReturnType<typeof vi.spyOn>;

	beforeEach(() => {
		consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
		document.body.innerHTML = '<div id="root"></div>';
		sessionStorage.clear();
	});

	afterEach(() => {
		consoleError.mockRestore();
		document.body.innerHTML = "";
	});

	it("turns a React render failure into the project recovery screen", () => {
		render(
			<FrameworkErrorBoundary>
				<ThrowOnRender message="Minified React error #185" />
			</FrameworkErrorBoundary>,
		);

		expect(
			screen.getByRole("heading", { name: "The app hit a runtime error" }),
		).toBeInTheDocument();
		expect(screen.getByTestId("framework-error-category")).toHaveTextContent(
			"React runtime failure",
		);
	});

	it("uses the document fallback when the recovery boundary itself fails", async () => {
		render(
			<DocumentFallbackBoundary>
				<ThrowOnRender message="bootstrap failed" />
			</DocumentFallbackBoundary>,
		);

		await waitFor(() => {
			expect(
				document.querySelector("[data-xp-document-fallback]"),
			).toBeInTheDocument();
		});
		expect(document.querySelector("[data-action=reload]")).toBeInTheDocument();
	});

	it("does not replace a mounted React app for later window errors", () => {
		const rootElement = document.querySelector("#root") as HTMLElement;
		rootElement.dataset.xpReactReady = "true";
		const cleanup = installDocumentFallbackHandlers(rootElement);

		window.dispatchEvent(new ErrorEvent("error", { error: new Error("late") }));
		expect(document.querySelector("[data-xp-document-fallback]")).toBeNull();
		cleanup();
	});
});
