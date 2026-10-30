import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";
import { Slider } from "./slider";

function SliderExample({ range = false }: { range?: boolean }) {
	const [value, setValue] = useState(range ? [16, 32] : [24]);
	return (
		<div className="flex w-full max-w-sm flex-col gap-4 p-6">
			<p className="text-sm font-medium">
				{range ? "Concurrency range" : "Connection limit"}
			</p>
			<Slider
				label={range ? "Concurrency" : "Connections"}
				value={value}
				max={64}
				onValueChange={setValue}
			/>
			<output className="text-sm text-muted-foreground">
				{value.join("–")}
			</output>
		</div>
	);
}

const meta = {
	title: "Components/ui/Slider",
	component: SliderExample,
	tags: ["autodocs"],
} satisfies Meta<typeof SliderExample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Single: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const slider = canvas.getByRole("slider", { name: "Connections" });
		slider.focus();
		await userEvent.keyboard("{ArrowRight}");
		await expect(canvas.getByText("25")).toBeVisible();
	},
};
export const Range: Story = { args: { range: true } };
