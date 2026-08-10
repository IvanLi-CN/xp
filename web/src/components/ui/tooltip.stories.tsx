import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, waitFor } from "@storybook/test";

import { Button } from "./button";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "./tooltip";

function TooltipExample() {
	return (
		<TooltipProvider delayDuration={0}>
			<Tooltip>
				<TooltipTrigger asChild>
					<Button variant="outline">Inspect full value</Button>
				</TooltipTrigger>
				<TooltipContent side="right" collisionPadding={12}>
					singapore-reality-edge-with-an-intentionally-long-descriptive-name
				</TooltipContent>
			</Tooltip>
		</TooltipProvider>
	);
}

const meta = {
	title: "UI/Tooltip",
	component: TooltipExample,
	tags: ["autodocs", "coverage-ui", "resource-navigation-polish"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component: [
					"Project tooltip primitive for concise, accessible supplemental values.",
					"Visible full-value hints use this component instead of browser title UI.",
				].join(" "),
			},
		},
	},
} satisfies Meta<typeof TooltipExample>;

export default meta;

type Story = StoryObj<typeof meta>;

export const RightSideWithCollisionFallback: Story = {
	play: async ({ canvasElement }) => {
		const trigger = canvasElement.querySelector("button");
		if (!trigger) throw new Error("Tooltip trigger is missing.");
		await userEvent.hover(trigger);
		await expect(await screen.findByRole("tooltip")).toHaveTextContent(
			"singapore-reality-edge",
		);
		await userEvent.keyboard("{Escape}");
		await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
	},
};
