import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import { BackendApiError } from "../api/backendError";
import { ReadStateBanner } from "./ReadStateBanner";

const meta: Meta<typeof ReadStateBanner> = {
	title: "Components/ReadStateBanner",
	component: ReadStateBanner,
	tags: ["autodocs", "coverage-ui"],
	args: {
		tone: "warning",
		title: "Offline read-only mode is active",
		description:
			"Showing the most recently synced cluster snapshot. Writes stay " +
			"disabled until the connection returns.",
	},
	decorators: [
		(Story) => (
			<div className="flex min-h-[calc(100vh-2rem)] items-center bg-background p-6">
				<div className="xp-card mx-auto flex min-h-72 w-full max-w-4xl items-center p-6">
					<Story />
				</div>
			</div>
		),
	],
};

export default meta;

type Story = StoryObj<typeof ReadStateBanner>;

export const OfflineSnapshot: Story = {};

export const Reconnecting: Story = {
	args: {
		title: "Live status stream is reconnecting",
		description:
			"The dashboard stays on the latest cached status until the next snapshot arrives.",
	},
};

export const InlineDismissible: Story = {
	args: {
		variant: "inline",
		dismissible: true,
		title: "Offline node snapshot",
		description: "Last successful sync: 2026/07/08 16:34:04.",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", {
				name: /dismiss offline node snapshot/i,
			}),
		);
		await expect(
			canvas.queryByText(/offline node snapshot/i),
		).not.toBeInTheDocument();
	},
};

export const UnauthorizedCachedData: Story = {
	args: {
		tone: "info",
		variant: "inline",
		dismissible: true,
		title: "Showing cached node inventory",
		description: "Last successful sync: 2026/07/30 11:41:56.",
		error: new BackendApiError({
			status: 401,
			message: "missing or invalid authorization token",
		}),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const signInLink = await canvas.findByRole("link", {
			name: /sign in again/i,
		});
		await expect(signInLink).toHaveAttribute(
			"href",
			expect.stringContaining("/login?redirect="),
		);
	},
};
