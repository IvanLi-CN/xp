import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import {
	type ModuleTabOption,
	ModuleTabsLayout,
	ModuleTabsPanel,
} from "./ModuleTabsLayout";

const options: ModuleTabOption[] = [
	{ value: "overview", label: "Overview" },
	{ value: "settings", label: "Settings" },
	{ value: "activity", label: "Activity" },
];

function ControlledModuleTabs() {
	const [value, setValue] = useState("overview");
	return (
		<ModuleTabsLayout
			ariaLabel="Module sections"
			options={options}
			value={value}
			onValueChange={setValue}
		>
			<ModuleTabsPanel value="overview">
				<section className="border-t border-border/70 pt-4">
					<h2 className="text-lg font-semibold">Overview</h2>
					<p className="mt-1 text-sm text-muted-foreground">
						Current module summary.
					</p>
				</section>
			</ModuleTabsPanel>
			<ModuleTabsPanel value="settings">
				<section className="border-t border-border/70 pt-4">
					<h2 className="text-lg font-semibold">Settings</h2>
				</section>
			</ModuleTabsPanel>
			<ModuleTabsPanel value="activity">
				<section className="border-t border-border/70 pt-4">
					<h2 className="text-lg font-semibold">Activity</h2>
				</section>
			</ModuleTabsPanel>
		</ModuleTabsLayout>
	);
}

const meta = {
	title: "Components/ModuleTabsLayout",
	component: ModuleTabsLayout,
	tags: ["autodocs", "coverage-ui"],
	parameters: { layout: "padded" },
	render: () => (
		<div className="h-[143px] w-[768px] max-w-full rounded-lg bg-muted p-6">
			<ControlledModuleTabs />
		</div>
	),
	args: {
		options,
		value: "overview",
		onValueChange: () => undefined,
		ariaLabel: "Module sections",
		children: null,
	},
} satisfies Meta<typeof ModuleTabsLayout>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByRole("tab", { name: "Settings" }));
		await expect(canvas.getByRole("tab", { name: "Settings" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		await expect(
			canvas.getByRole("heading", { name: "Settings" }),
		).toBeInTheDocument();
	},
};
