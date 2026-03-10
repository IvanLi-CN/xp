import type { Meta, StoryObj } from "@storybook/react";

import { Input } from "./input";
import { Label } from "./label";

const meta = {
	title: "UI/Label",
	component: Label,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Label primitive for form controls. Even though it is intentionally thin, a dedicated story helps keep control pairing and disabled-peer styling visible in docs.",
			},
		},
	},
} satisfies Meta<typeof Label>;

export default meta;

type Story = StoryObj<typeof meta>;

export const PairedControl: Story = {
	render: () => (
		<div className="grid w-[320px] gap-2">
			<Label htmlFor="storybook-label-input">Display name</Label>
			<Input id="storybook-label-input" defaultValue="Tokyo" />
		</div>
	),
};
