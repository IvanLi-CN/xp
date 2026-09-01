import type { Meta, StoryObj } from "@storybook/react";

import { ServiceMonitorStatusBadge } from "./ServiceMonitorStatusBadge";

const meta = {
	title: "Components/ServiceMonitorStatusBadge",
	component: ServiceMonitorStatusBadge,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div className="grid min-h-32 place-items-center bg-background p-8">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof ServiceMonitorStatusBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Up: Story = { args: { status: "up" } };
export const Degraded: Story = { args: { status: "degraded" } };
export const Down: Story = { args: { status: "down" } };
export const Stale: Story = { args: { status: "up", stale: true } };
export const CaptureSuspended: Story = {
	args: { status: "capture_suspended" },
};
