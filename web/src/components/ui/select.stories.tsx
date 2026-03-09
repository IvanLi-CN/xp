import type { Meta, StoryObj } from "@storybook/react";

import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "./select";

const meta = {
	title: "UI/Select",
	component: Select,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Select primitive for admin filters and RHF-backed forms. Use the open story to verify menu spacing, focus styles, and long labels under both theme and density combinations.",
			},
		},
	},
} satisfies Meta<typeof Select>;

export default meta;

type Story = StoryObj<typeof meta>;

function SelectDemo(props: {
	defaultValue?: string;
	disabled?: boolean;
	open?: boolean;
}) {
	return (
		<Select
			defaultValue={props.defaultValue}
			disabled={props.disabled}
			open={props.open}
		>
			<SelectTrigger className="w-[240px]" aria-label="Quota reset policy">
				<SelectValue placeholder="Choose a policy" />
			</SelectTrigger>
			<SelectContent>
				<SelectItem value="monthly">Monthly reset</SelectItem>
				<SelectItem value="weekly">Weekly reset</SelectItem>
				<SelectItem value="unlimited">Unlimited</SelectItem>
			</SelectContent>
		</Select>
	);
}

export const Placeholder: Story = {
	render: () => <SelectDemo />,
};

export const Selected: Story = {
	render: () => <SelectDemo defaultValue="monthly" />,
};

export const OpenMenu: Story = {
	render: () => <SelectDemo defaultValue="monthly" open />,
};
