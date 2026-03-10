import type { Meta, StoryObj } from "@storybook/react";

import { Textarea } from "./textarea";

const meta = {
	title: "UI/Textarea",
	component: Textarea,
	tags: ["autodocs", "coverage-ui"],
	args: {
		placeholder: "Paste YAML or JSON payload...",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Multi-line text input primitive for config editors, annotations, and notes. Stories keep one empty state and one filled state so line height and border treatments are easy to compare across themes.",
			},
		},
	},
} satisfies Meta<typeof Textarea>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Empty: Story = {
	render: (args) => <Textarea className="w-[360px]" {...args} />,
};

export const Filled: Story = {
	render: (args) => (
		<Textarea
			className="w-[360px]"
			defaultValue={`inbounds:
  - type: vless
    tag: tokyo-main`}
			{...args}
		/>
	),
};
