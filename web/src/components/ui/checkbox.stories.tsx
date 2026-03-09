import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { Checkbox } from "./checkbox";
import { Label } from "./label";

const meta = {
	title: "UI/Checkbox",
	component: Checkbox,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Checkbox primitive for settings toggles and editor locks. Stories cover direct labeling and disabled states so the Radix button-backed control stays accessible in docs and tests.",
			},
		},
	},
} satisfies Meta<typeof Checkbox>;

export default meta;

type Story = StoryObj<typeof meta>;

function CheckboxField(props: {
	defaultChecked?: boolean;
	disabled?: boolean;
	label: string;
	description?: string;
}) {
	const [checked, setChecked] = useState(props.defaultChecked ?? false);
	return (
		<div className="flex items-start gap-3 rounded-2xl border border-border/70 p-4">
			<Checkbox
				id="storybook-checkbox"
				checked={checked}
				disabled={props.disabled}
				onCheckedChange={(next) => setChecked(next === true)}
			/>
			<div className="space-y-1">
				<Label htmlFor="storybook-checkbox">{props.label}</Label>
				{props.description ? (
					<p className="text-sm text-muted-foreground">{props.description}</p>
				) : null}
			</div>
		</div>
	);
}

export const Unchecked: Story = {
	render: () => (
		<CheckboxField
			label="Automatic updates"
			description="Run the managed DB-IP Lite refresh worker on every node."
		/>
	),
};

export const Checked: Story = {
	render: () => (
		<CheckboxField
			defaultChecked
			label="Inherit global default ratios"
			description="Node editor stays read-only until the toggle is cleared."
		/>
	),
};

export const Disabled: Story = {
	render: () => (
		<CheckboxField
			defaultChecked
			disabled
			label="Locked"
			description="Shown while a save is in flight."
		/>
	),
};

export const Indeterminate: Story = {
	render: () => (
		<div className="flex items-start gap-3 rounded-2xl border border-border/70 p-4">
			<Checkbox id="storybook-checkbox-indeterminate" checked="indeterminate" />
			<div className="space-y-1">
				<Label htmlFor="storybook-checkbox-indeterminate">
					Partially applied
				</Label>
				<p className="text-sm text-muted-foreground">
					Mixed selections surface the indeterminate affordance used by access
					matrices and bulk actions.
				</p>
			</div>
		</div>
	),
};
