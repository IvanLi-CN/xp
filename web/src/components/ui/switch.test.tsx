import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Switch } from "./switch";

describe("<Switch />", () => {
	it("uses the shared compact control contract and emits the next value", () => {
		const onCheckedChange = vi.fn();
		render(
			<Switch
				aria-label="Enable feature"
				checked
				onCheckedChange={onCheckedChange}
			/>,
		);

		const control = screen.getByRole("switch", { name: "Enable feature" });
		expect(control).toHaveAttribute("aria-checked", "true");
		expect(control).toHaveClass(
			"h-6",
			"w-11",
			"border-2",
			"border-transparent",
			"focus-visible:ring-[3px]",
			"focus-visible:ring-ring/20",
			"motion-reduce:transition-none",
			"hover:bg-primary/90",
		);

		fireEvent.click(control);
		expect(onCheckedChange).toHaveBeenCalledWith(false);
	});

	it("does not emit changes when disabled", () => {
		const onCheckedChange = vi.fn();
		render(
			<Switch
				aria-label="Enable feature"
				disabled
				onCheckedChange={onCheckedChange}
			/>,
		);

		const control = screen.getByRole("switch", { name: "Enable feature" });
		expect(control).toBeDisabled();
		expect(control).toHaveClass(
			"disabled:cursor-not-allowed",
			"disabled:opacity-50",
		);

		fireEvent.click(control);
		expect(onCheckedChange).not.toHaveBeenCalled();
	});
});
