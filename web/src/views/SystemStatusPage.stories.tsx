import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";

import { demoMeshStatus } from "@/demo/DemoSystemStatusPage";

import { SystemStatusSurface } from "./SystemStatusPage";

const meta = {
	title: "Views/SystemStatusPage",
	component: SystemStatusSurface,
	tags: ["autodocs", "coverage-ui", "system-status"],
	parameters: { layout: "fullscreen" },
} satisfies Meta<typeof SystemStatusSurface>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Healthy: Story = {
	args: {
		status: demoMeshStatus,
		components: [
			{ component: "xp", status: "up" },
			{ component: "xray", status: "up" },
			{ component: "cloudflared", status: "up" },
			{ component: "ddns", status: "up" },
		],
		onProbeAll: fn(),
		onProbePeer: fn(),
	},
	play: async ({ canvasElement, args }) => {
		const canvas = within(canvasElement);
		const refreshButton = await canvas.findByRole("button", {
			name: "Refresh",
		});
		const probeAllButton = await canvas.findByRole("button", {
			name: "Probe all",
		});
		await expect(refreshButton).toHaveClass("w-28");
		await expect(probeAllButton).toHaveClass("w-28");
		await expect(
			await canvas.findByRole("heading", { name: "Peer transport" }),
		).toBeInTheDocument();
		const probe = await canvas.findByRole("button", {
			name: `Probe ${demoMeshStatus.peers[0].node_name}`,
		});
		const details = await canvas.findByRole("link", {
			name: `Open ${demoMeshStatus.peers[0].node_name} details`,
		});
		await expect(probe).toHaveClass("size-8", "min-h-8", "min-w-8", "p-0");
		await expect(details).toHaveClass("size-8", "min-h-8", "min-w-8", "p-0");
		await expect(probe).toHaveAttribute(
			"title",
			`Probe ${demoMeshStatus.peers[0].node_name}`,
		);
		await expect(details).toHaveAttribute(
			"title",
			`Open ${demoMeshStatus.peers[0].node_name} details`,
		);
		await expect(
			(await canvas.findAllByText("Mesh available")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Using public fallback")).length,
		).toBeGreaterThan(0);
		await userEvent.click(probe);
		await expect(args.onProbePeer).toHaveBeenCalledWith(
			demoMeshStatus.peers[0].node_id,
		);
		await userEvent.click(probeAllButton);
		await expect(args.onProbeAll).toHaveBeenCalledOnce();
	},
};

export const Empty: Story = {
	args: {
		status: { ...demoMeshStatus, peers: [], events: [] },
		components: [],
	},
};

export const PartialAndFallback: Story = {
	args: {
		status: {
			...demoMeshStatus,
			peers: demoMeshStatus.peers.map((peer, index) =>
				index === 0
					? {
							...peer,
							current_path: "public" as const,
							quality: "slow" as const,
						}
					: peer,
			),
		},
	},
};

export const Down: Story = {
	args: {
		status: {
			...demoMeshStatus,
			peers: demoMeshStatus.peers.map((peer, index) =>
				index === 1
					? {
							...peer,
							current_path: "public" as const,
							quality: "down" as const,
							availability_1h: 0,
							availability_24h: 0.72,
						}
					: peer,
			),
		},
	},
};

export const CanaryDegraded: Story = {
	args: {
		status: {
			...demoMeshStatus,
			local: {
				...demoMeshStatus.local,
				canary: {
					...demoMeshStatus.local.canary,
					last_error: "certificate renewal is waiting for DNS propagation",
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByText("canary")).toBeInTheDocument();
		await expect(await canvas.findByText("degraded")).toBeInTheDocument();
	},
};

export const OfflineReadOnly: Story = {
	args: {
		status: demoMeshStatus,
		readOnly: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("button", { name: "Probe all" }),
		).toBeDisabled();
		await expect(
			await canvas.findByRole("button", {
				name: `Probe ${demoMeshStatus.peers[0].node_name}`,
			}),
		).toBeDisabled();
		await expect(
			await canvas.findByRole("link", {
				name: `Open ${demoMeshStatus.peers[0].node_name} details`,
			}),
		).toBeInTheDocument();
	},
};

export const Stale: Story = {
	args: {
		status: {
			...demoMeshStatus,
			peers: demoMeshStatus.peers.map((peer, index) =>
				index === 0
					? {
							...peer,
							quality: "unknown" as const,
							stale: true,
						}
					: peer,
			),
		},
	},
};

export const FiftyPeers: Story = {
	args: {
		status: {
			...demoMeshStatus,
			peers: Array.from({ length: 50 }, (_, index) => {
				const source =
					demoMeshStatus.peers[index % demoMeshStatus.peers.length];
				return {
					...source,
					node_id: `${source.node_id}-${index + 1}`,
					node_name: `${source.node_name}-${index + 1}`,
				};
			}),
		},
	},
};
