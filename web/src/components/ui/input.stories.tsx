import type { Meta, StoryObj } from "@storybook/react";

import { Input } from "./input";

const meta = {
	title: "UI/Input",
	component: Input,
	tags: ["autodocs", "coverage-ui"],
	args: {
		placeholder: "Search nodes, endpoints, or users",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Single-line input primitive shared by filters, forms, and command surfaces. Use the toolbar to compare comfortable vs compact density because the wrapper components inherit this control directly.",
			},
		},
	},
} satisfies Meta<typeof Input>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	render: (args) => <Input className="w-[320px]" {...args} />,
};

export const Invalid: Story = {
	render: (args) => (
		<Input
			className="w-[320px]"
			aria-invalid
			defaultValue="not-a-valid-domain"
			{...args}
		/>
	),
	args: {
		placeholder: "api.example.com",
	},
};

export const Disabled: Story = {
	render: (args) => (
		<Input
			className="w-[320px]"
			disabled
			defaultValue="leader-token"
			{...args}
		/>
	),
};
