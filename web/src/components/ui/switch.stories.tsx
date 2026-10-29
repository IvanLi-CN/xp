import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { useState } from "react";

import { Switch } from "./switch";

const meta = {
	title: "UI/Switch",
	component: Switch,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Shared compact switch for boolean settings. The control keeps " +
					"the project focus, hover, disabled, and theme tokens in one place.",
			},
		},
	},
} satisfies Meta<typeof Switch>;

export default meta;
type Story = StoryObj<typeof meta>;

function SwitchField(props: {
	defaultChecked?: boolean;
	disabled?: boolean;
	indeterminate?: boolean;
	label: string;
	description: string;
}) {
	const [checked, setChecked] = useState(props.defaultChecked ?? false);
	return (
		<div className="flex items-start gap-3 rounded-2xl border border-border/70 p-4">
			<Switch
				aria-label={props.label}
				checked={checked}
				disabled={props.disabled}
				indeterminate={props.indeterminate}
				onCheckedChange={setChecked}
			/>
			<div>
				<p className="font-semibold">{props.label}</p>
				<p className="mt-1 text-sm text-muted-foreground">
					{props.description}
				</p>
			</div>
		</div>
	);
}

async function expectThumbSlot(control: HTMLElement, checked: boolean) {
	await new Promise((resolve) => setTimeout(resolve, 200));
	const thumb = control.firstElementChild;
	if (!(thumb instanceof HTMLElement)) {
		throw new Error("Switch thumb is missing");
	}
	const trackRect = control.getBoundingClientRect();
	const thumbRect = thumb.getBoundingClientRect();
	const leftInset = thumbRect.left - trackRect.left;
	const rightInset = trackRect.right - thumbRect.right;
	expect(leftInset).toBeCloseTo(checked ? 22 : 2, 0);
	expect(rightInset).toBeCloseTo(checked ? 2 : 22, 0);
}

export const Unchecked: Story = {
	render: () => (
		<SwitchField
			label="Automatic updates"
			description="Run the refresh worker on every node."
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const control = canvas.getByRole("switch", {
			name: "Automatic updates",
		});
		await expect(control).toHaveAttribute("aria-checked", "false");
		await expectThumbSlot(control, false);
		await control.click();
		await expect(control).toHaveAttribute("aria-checked", "true");
		await expectThumbSlot(control, true);
	},
};

export const Checked: Story = {
	render: () => (
		<SwitchField
			defaultChecked
			label="Enable Reality Direct Mesh"
			description="Use the cluster-wide signed control-plane path."
		/>
	),
};

export const Indeterminate: Story = {
	render: () => (
		<SwitchField
			indeterminate
			label="Unknown observation"
			description="The cluster value is not safe to interpret as enabled or disabled."
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const control = canvas.getByRole("button", {
			name: "Unknown observation",
		});
		await expect(control).not.toHaveAttribute("aria-checked");
		await expect(control).toHaveAttribute("aria-describedby");
	},
};

export const Disabled: Story = {
	render: () => (
		<SwitchField
			defaultChecked
			disabled
			label="Locked setting"
			description="Changes are unavailable while the observation is unknown."
		/>
	),
};
