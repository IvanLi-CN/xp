import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

import { BackendApiError } from "../api/backendError";
import { AuthRecoveryAction } from "./AuthRecoveryAction";

const meta = {
	title: "Components/AuthRecoveryAction",
	component: AuthRecoveryAction,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div className="flex min-h-32 items-center justify-center bg-muted/35 p-6">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof AuthRecoveryAction>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Unauthorized: Story = {
	args: {
		error: new BackendApiError({
			status: 401,
			code: "unauthorized",
			message: "missing or invalid authorization token",
		}),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("link", { name: "Sign in again" }),
		).toHaveAttribute("href", expect.stringContaining("/login?redirect="));
	},
};

export const Forbidden: Story = {
	args: {
		error: new BackendApiError({
			status: 403,
			code: "forbidden",
			message: "access denied",
		}),
	},
};
