import type { Meta, StoryObj } from "@storybook/react";

import { BackendApiError } from "../api/backendError";
import { Button } from "./Button";
import { CapabilityUnavailableState, PageState } from "./PageState";

const meta: Meta<typeof PageState> = {
	title: "Components/PageState",
	component: PageState,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div className="flex min-h-[calc(100vh-2rem)] items-center bg-primary/20 p-6">
				<div className="mx-auto flex min-h-72 w-full max-w-3xl items-center bg-card p-6">
					<Story />
				</div>
			</div>
		),
	],
	args: {
		title: "Title",
		description: "Description",
	},
};

export default meta;

type Story = StoryObj<typeof PageState>;

export const Loading: Story = {
	args: { variant: "loading", title: "Loading" },
};

export const Empty: Story = {
	args: { variant: "empty", title: "No data" },
};

export const ErrorState: Story = {
	args: { variant: "error", title: "Something went wrong" },
};

export const Unauthorized: Story = {
	args: {
		variant: "error",
		title: "Failed to load nodes",
		description: "401 unauthorized: missing or invalid authorization token",
		error: new BackendApiError({
			status: 401,
			code: "unauthorized",
			message: "missing or invalid authorization token",
		}),
		action: <Button variant="secondary">Retry</Button>,
	},
};

export const Offline: Story = {
	args: {
		variant: "offline",
		title: "Offline cache unavailable",
		description: "Reconnect to hydrate this page for offline use.",
	},
};

export const CapabilityUnavailable: Story = {
	render: () => (
		<CapabilityUnavailableState
			title="Mesh status unavailable"
			reason="The connected API does not advertise admin.mesh."
		/>
	),
};
