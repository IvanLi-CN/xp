import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DemoFrameworkRecoveryPage } from "./DemoFrameworkRecoveryPage";

describe("DemoFrameworkRecoveryPage", () => {
	afterEach(() => {
		window.history.replaceState({}, "", "/");
	});

	it("renders the recovery surface without demo controls", () => {
		window.history.replaceState({}, "", "/ui-demo/framework-recovery");

		render(<DemoFrameworkRecoveryPage />);

		expect(
			screen.getByRole("heading", {
				name: "The page bundle could not be loaded",
			}),
		).toBeInTheDocument();
		expect(screen.queryByText("Error category")).not.toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Toggle repeated state" }),
		).not.toBeInTheDocument();
	});

	it("selects a deterministic repeated-failure scenario from the URL", () => {
		window.history.replaceState(
			{},
			"",
			"/ui-demo/framework-recovery?category=react-runtime&repeat=1",
		);

		render(<DemoFrameworkRecoveryPage />);

		expect(
			screen.getByRole("heading", { name: "The app hit a runtime error" }),
		).toBeInTheDocument();
		expect(screen.getByTestId("repeat-failure")).toBeInTheDocument();
	});
});
