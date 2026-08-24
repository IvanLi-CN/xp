import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import {
	type ModuleTabOption,
	ModuleTabsLayout,
	ModuleTabsPanel,
} from "./ModuleTabsLayout";

const options: ModuleTabOption[] = [
	{ value: "nodes", label: "Nodes" },
	{ value: "join", label: "Join node" },
];

function Harness() {
	const [value, setValue] = useState("nodes");
	return (
		<ModuleTabsLayout
			options={options}
			value={value}
			onValueChange={setValue}
			ariaLabel="Module sections"
		>
			<ModuleTabsPanel value="nodes">Node content</ModuleTabsPanel>
			<ModuleTabsPanel value="join">Join content</ModuleTabsPanel>
		</ModuleTabsLayout>
	);
}

describe("ModuleTabsLayout", () => {
	it("uses one controlled value for desktop tabs and the mobile select", async () => {
		const user = userEvent.setup();
		Object.assign(HTMLElement.prototype, {
			hasPointerCapture: () => false,
			releasePointerCapture: () => undefined,
			setPointerCapture: () => undefined,
		});
		render(<Harness />);

		expect(screen.getByRole("tab", { name: "Nodes" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		expect(screen.getByText("Node content")).toBeVisible();

		await user.click(screen.getByRole("tab", { name: "Join node" }));
		expect(screen.getByRole("tab", { name: "Join node" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		expect(screen.getByText("Join content")).toBeVisible();

		await user.click(screen.getByRole("combobox", { name: "Module sections" }));
		await user.click(screen.getByRole("option", { name: "Nodes" }));
		expect(screen.getByRole("tab", { name: "Nodes" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		expect(screen.getByText("Node content")).toBeVisible();
	});

	it("preserves keyboard tab semantics", async () => {
		const user = userEvent.setup();
		render(<Harness />);

		const nodesTab = screen.getByRole("tab", { name: "Nodes" });
		await user.click(nodesTab);
		await user.keyboard("{ArrowRight}");

		expect(screen.getByRole("tab", { name: "Join node" })).toHaveFocus();
	});

	it("emits one change for a desktop click", async () => {
		const user = userEvent.setup();
		const onValueChange = vi.fn();
		render(
			<ModuleTabsLayout
				options={options}
				value="nodes"
				onValueChange={onValueChange}
				ariaLabel="Module sections"
			>
				<ModuleTabsPanel value="nodes">Node content</ModuleTabsPanel>
				<ModuleTabsPanel value="join">Join content</ModuleTabsPanel>
			</ModuleTabsLayout>,
		);

		await user.click(screen.getByRole("tab", { name: "Join node" }));
		expect(onValueChange).toHaveBeenCalledTimes(1);
		expect(onValueChange).toHaveBeenCalledWith("join");
	});
});
