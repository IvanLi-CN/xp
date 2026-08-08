import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { demoMeshStatus } from "@/demo/DemoSystemStatusPage";

import { SystemStatusSurface } from "./SystemStatusPage";

const meta = {
	title: "Views/SystemStatusPage",
	component: SystemStatusSurface,
	tags: ["autodocs", "coverage-ui", "system-status"],
	parameters: { layout: "fullscreen" },
	args: { showMeshTransportReuse: true },
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
		const probeRect = probe.getBoundingClientRect();
		const detailsRect = details.getBoundingClientRect();
		const peerRow = details.closest("[data-peer-row]");
		await expect(peerRow).not.toBeNull();
		const rowRect = peerRow?.getBoundingClientRect();
		await expect(probeRect.width).toBe(32);
		await expect(probeRect.height).toBe(32);
		await expect(detailsRect.width).toBe(probeRect.width);
		await expect(detailsRect.height).toBe(probeRect.height);
		await expect(detailsRect.right).toBeLessThanOrEqual(rowRect?.right ?? 0);
		await expect(
			(await canvas.findAllByText("Mesh available")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Using public fallback")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("H2 · 58 req / 1 starts · gen 3")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Churning · H2 · 17 req / 4 starts · gen 18"))
				.length,
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

export const ReuseUnavailable: Story = {
	args: {
		status: {
			...demoMeshStatus,
			peers: demoMeshStatus.peers.map((peer, index) =>
				index === 0 ? { ...peer, mesh_transport: undefined } : peer,
			),
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			(await canvas.findAllByText("Reuse data unavailable")).length,
		).toBeGreaterThan(0);
	},
};

export const LegacyBackend: Story = {
	args: {
		status: demoMeshStatus,
		showMeshTransportReuse: false,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.queryByText("H2 · 58 req / 1 starts · gen 3"),
		).not.toBeInTheDocument();
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
							availability_1h: fixtureCatalog.slotNumber.n16(),
							availability_24h: fixtureCatalog.slotNumber.n17(),
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
					node_id: fixtureCatalog.slotString.s238(),
					node_name: fixtureCatalog.slotString.s239(),
				};
			}),
		},
	},
	play: async ({ canvasElement }) => {
		const rows = canvasElement.querySelectorAll("[data-peer-row]");
		await expect(rows).toHaveLength(50);
		await expect(canvasElement.scrollWidth).toBeLessThanOrEqual(
			canvasElement.clientWidth,
		);
	},
};
