import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ConfirmDialog } from "./ConfirmDialog";

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("<ConfirmDialog />", () => {
	it("calls onCancel once when cancel is pressed", () => {
		const onCancel = vi.fn();
		render(
			<ConfirmDialog
				open
				title="Delete endpoint"
				description="This action cannot be undone."
				onCancel={onCancel}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		expect(onCancel).toHaveBeenCalledTimes(1);
	});

	it("keeps the dialog open when confirm is pressed and avoids cancel callbacks", () => {
		const onConfirm = vi.fn();
		const onCancel = vi.fn();
		render(
			<ConfirmDialog
				open
				title="Reset token"
				description="This invalidates the old token immediately."
				onConfirm={onConfirm}
				onCancel={onCancel}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Confirm" }));

		expect(onConfirm).toHaveBeenCalledTimes(1);
		expect(onCancel).not.toHaveBeenCalled();
		expect(screen.getByRole("alertdialog")).toBeTruthy();
	});

	it("suppresses the missing-description warning when description is omitted", () => {
		const consoleError = vi
			.spyOn(console, "error")
			.mockImplementation(() => undefined);
		render(<ConfirmDialog open title="Delete endpoint" />);

		expect(consoleError).not.toHaveBeenCalled();
	});
});
