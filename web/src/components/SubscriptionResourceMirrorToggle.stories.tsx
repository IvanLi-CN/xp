import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { SubscriptionResourceMirrorToggle } from "./SubscriptionResourceMirrorToggle";

const meta = {
	title: "Components/SubscriptionResourceMirrorToggle",
	component: SubscriptionResourceMirrorToggle,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Temporary Mihomo-only delivery toggle. The unchecked state keeps original resource URLs; " +
					"the checked state opts into XP's scoped mirror URLs.",
			},
		},
	},
} satisfies Meta<typeof SubscriptionResourceMirrorToggle>;

export default meta;
type Story = StoryObj<typeof SubscriptionResourceMirrorToggle>;

export const Interactive: Story = {
	render: () => {
		const [checked, setChecked] = useState(false);
		return (
			<SubscriptionResourceMirrorToggle
				checked={checked}
				onCheckedChange={setChecked}
			/>
		);
	},
};

export const Checked: Story = {
	render: () => (
		<SubscriptionResourceMirrorToggle checked onCheckedChange={() => {}} />
	),
};
