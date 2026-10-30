import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";
import { EndpointRealityFingerprintSettings } from "./EndpointRealityFingerprintSettings";

function FingerprintStory({ disabled = false }: { disabled?: boolean }) {
	const [value, setValue] = useState("chrome");
	return (
		<EndpointRealityFingerprintSettings
			value={value}
			onChange={setValue}
			disabled={disabled}
			inputClass=""
		/>
	);
}
const meta = {
	title: "Components/EndpointRealityFingerprintSettings",
	component: FingerprintStory,
	tags: ["autodocs"],
} satisfies Meta<typeof FingerprintStory>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Editable: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByText("Advanced (optional)"));
		await expect(canvas.getByLabelText("fingerprint")).toHaveValue("chrome");
	},
};
export const Disabled: Story = { args: { disabled: true } };
