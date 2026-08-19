import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { fixtureStoryData } from "../fixture-policy/storybook";

import {
	demoMeshStatus,
	demoReverseMeshStatus,
} from "@/demo/DemoSystemStatusPage";

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
			(await canvas.findAllByText("Reality direct")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Public fallback")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("H2 · 58 req · gen 3")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("H2 churning · 17 req · gen 18")).length,
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
			canvas.queryByText("H2 · 58 req · gen 3"),
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

export const ReverseRelay: Story = {
	parameters: {
		viewport: {
			defaultViewport: "reverseRelayDesktop",
			viewports: {
				reverseRelayDesktop: {
					name: "Reverse relay desktop (1280x900)",
					styles: { width: "1280px", height: "900px" },
					type: "desktop",
				},
				reverseRelayMobile: {
					name: "Reverse relay mobile (393x852)",
					styles: { width: "393px", height: "852px" },
					type: "mobile",
				},
			},
		},
	},
	args: {
		status: demoReverseMeshStatus,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByText("Cluster nodes")).toBeInTheDocument();
		await expect(
			await canvas.findByText("1 local · 4 remote"),
		).toBeInTheDocument();
		for (const nodeId of [
			"node-sgp-1",
			"node-syd-1",
			"node-osaka-1",
			"node-seoul-1",
		]) {
			await expect(
				canvasElement.querySelector(`[data-peer-row="${nodeId}"]`),
			).not.toBeNull();
		}
		await expect(
			(await canvas.findAllByText("Rendezvous · primary")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Rendezvous · standby")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("Reverse relay")).length,
		).toBeGreaterThan(0);
		await expect(
			(await canvas.findAllByText("via singapore-1 · g7")).length,
		).toBeGreaterThan(0);
		await expect(
			canvas.queryByText("Primary · standby sydney-1"),
		).not.toBeInTheDocument();
		const peerRows = canvasElement.querySelectorAll("[data-peer-row]");
		await expect(peerRows).toHaveLength(4);
		const routeLines = canvasElement.querySelectorAll("[data-peer-route-line]");
		await expect(routeLines).toHaveLength(16);
		for (const line of routeLines) {
			await expect(getComputedStyle(line).whiteSpace).toBe("nowrap");
		}
		for (const nodeId of ["node-osaka-1", "node-seoul-1"]) {
			await expect(
				canvasElement.querySelector(
					`[data-peer-row="${nodeId}"] [data-peer-cell="route"]`,
				)?.children.length,
			).toBe(2);
		}
		for (const cell of canvasElement.querySelectorAll("[data-peer-cell]")) {
			await expect(cell.children.length).toBeLessThanOrEqual(2);
			for (const line of cell.querySelectorAll(":scope > p")) {
				await expect(getComputedStyle(line).whiteSpace).toBe("nowrap");
			}
		}
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
							availability_1h: fixtureCatalog.number.value0(),
							availability_24h: fixtureCatalog.number.value0Point72(),
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
			peers: fixtureStoryData.fiftyMeshPeers(),
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
