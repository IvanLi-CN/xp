import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import { DEFAULT_SUBSCRIPTION_FORMAT } from "@/api/subscription";

import { SubscriptionFormatSegmentedControl } from "./SubscriptionFormatSegmentedControl";

function Demo() {
	const [value, setValue] = useState(DEFAULT_SUBSCRIPTION_FORMAT);
	return (
		<div className="max-w-xl rounded-2xl border border-border/70 bg-card p-4">
			<SubscriptionFormatSegmentedControl
				value={value}
				onValueChange={setValue}
				testId="story-subscription-format"
			/>
		</div>
	);
}

const meta = {
	title: "Components/SubscriptionFormatSegmentedControl",
	component: Demo,
	parameters: {
		layout: "centered",
	},
} satisfies Meta<typeof Demo>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const group = await canvas.findByRole("radiogroup", {
			name: "Subscription format",
		});
		await expect(within(group).getByLabelText("Raw")).toBeChecked();
		await userEvent.click(within(group).getByLabelText("Mihomo"));
		await expect(within(group).getByLabelText("Mihomo")).toBeChecked();
	},
};
