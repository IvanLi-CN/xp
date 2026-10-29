import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Alert, type AlertTone } from "./alert";

describe("Alert", () => {
	it.each<AlertTone>(["info", "warning", "success", "error"])(
		"renders the %s tone without an implicit live announcement",
		(tone) => {
			const { container } = render(<Alert tone={tone}>Notice</Alert>);
			expect(container.firstElementChild).toHaveClass(
				"xp-alert",
				`xp-alert-${tone}`,
			);
			expect(screen.queryByRole("alert")).toBeNull();
			expect(screen.queryByRole("status")).toBeNull();
		},
	);

	it("preserves requested announcement semantics and interactive content", () => {
		render(
			<Alert tone="warning" role="alert" className="flex-col">
				<p>Action needed</p>
				<button type="button">Retry</button>
			</Alert>,
		);
		expect(screen.getByRole("alert")).toHaveTextContent("Action needed");
		expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
	});
});
