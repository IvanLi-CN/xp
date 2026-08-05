import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import { AppShell } from "./AppShell";

const meta: Meta<typeof AppShell> = {
	title: "Components/AppShell",
	component: AppShell,
	tags: ["autodocs", "coverage-ui"],
	args: {
		brand: { name: "xp", subtitle: "cluster manager" },
		navItems: [
			{ label: "Dashboard", to: "/", icon: "tabler:layout-dashboard" },
			{ label: "Nodes", to: "/nodes", icon: "tabler:server" },
			{ label: "Endpoints", to: "/endpoints", icon: "tabler:plug" },
			{ label: "Users", to: "/users", icon: "tabler:users" },
		],
		children: (
			<div className="space-y-2">
				<h2 className="text-xl font-semibold">Content</h2>
				<p className="text-sm opacity-70">Rendered inside AppShell.</p>
			</div>
		),
	},
};

export default meta;

type Story = StoryObj<typeof AppShell>;

export const Default: Story = {};

export const OfflineTopbarIndicator: Story = {
	parameters: {
		appRuntime: {
			initialIsOnline: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/offline cached/i),
		).toBeInTheDocument();
	},
};

export const ApiCompatibilityDegraded: Story = {
	parameters: {
		mockApi: {
			data: {
				capabilities: {
					release_tag: "v3.22.5",
					capabilities: ["api.health", "api.cluster-info", "admin.nodes"],
					fingerprint: {},
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByRole("button", { name: "Open status" }));
		const page = within(canvasElement.ownerDocument.body);
		await expect(
			await page.findByTitle("Realtime status stream"),
		).toHaveTextContent("unavailable");
	},
};
