import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type {
	AdminEndpointProbeHistoryResponse,
	AdminEndpointProbeRunStatusResponse,
} from "../api/adminEndpointProbes";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";

const ENDPOINT_ID = fixtureCatalog.slotString.s120();

const nodes: AdminNode[] = [
	{
		node_id: fixtureCatalog.slotString.s206(),
		node_name: fixtureCatalog.slotString.s207(),
		api_base_url: fixtureCatalog.slotString.s208(),
		access_host: fixtureCatalog.slotString.s209(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: fixtureCatalog.slotString.s134(),
		node_name: fixtureCatalog.slotString.s210(),
		api_base_url: fixtureCatalog.slotString.s211(),
		access_host: fixtureCatalog.slotString.s212(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: fixtureCatalog.slotString.s213(),
		node_name: fixtureCatalog.slotString.s99(),
		api_base_url: fixtureCatalog.slotString.s214(),
		access_host: fixtureCatalog.slotString.s215(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
];

const endpoints: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.slotString.s120(),
		node_id: fixtureCatalog.slotString.s134(),
		tag: fixtureCatalog.slotString.s216(),
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {},
		probe: {
			latest_checked_at: fixtureCatalog.slotString.s217(),
			latest_latency_ms_p50: 32,
			slots: [
				{
					hour: fixtureCatalog.timestamp.probeHour(),
					status: "up",
					checked_at: fixtureCatalog.slotString.s217(),
					latency_ms_p50: 32,
				},
			],
		},
	},
];

const history: AdminEndpointProbeHistoryResponse = {
	endpoint_id: fixtureCatalog.slotString.s120(),
	participating_nodes: 3,
	slots: [
		{
			hour: fixtureCatalog.timestamp.probeHour(),
			status: "up",
			participating_nodes: 3,
			ok_count: 3,
			sample_count: 3,
			skipped_count: 0,
			tested_count: 3,
			latency_ms_p50: 32,
			latency_ms_p95: 76,
			by_node: [
				{
					node_id: fixtureCatalog.slotString.s134(),
					ok: true,
					checked_at: fixtureCatalog.slotString.s218(),
					latency_ms: fixtureCatalog.slotNumber.n9(),
					config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				},
				{
					node_id: fixtureCatalog.slotString.s206(),
					ok: true,
					checked_at: fixtureCatalog.slotString.s219(),
					latency_ms: fixtureCatalog.slotNumber.n10(),
					config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				},
				{
					node_id: fixtureCatalog.slotString.s213(),
					ok: true,
					checked_at: fixtureCatalog.slotString.s217(),
					latency_ms: fixtureCatalog.slotNumber.n11(),
					config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				},
			],
		},
	],
};

const run: AdminEndpointProbeRunStatusResponse = {
	run_id: fixtureCatalog.identifier.probeRunPrimary(),
	status: "finished",
	hour: fixtureCatalog.timestamp.probeHour(),
	config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
	nodes: [
		{
			node_id: fixtureCatalog.slotString.s134(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.slotString.s221(),
				finished_at: fixtureCatalog.slotString.s218(),
			},
		},
		{
			node_id: fixtureCatalog.slotString.s206(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.slotString.s221(),
				finished_at: fixtureCatalog.slotString.s219(),
			},
		},
		{
			node_id: fixtureCatalog.slotString.s213(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.slotString.s221(),
				finished_at: fixtureCatalog.slotString.s217(),
			},
		},
	],
};

function Empty() {
	return <></>;
}

const meta = {
	title: "Pages/EndpointProbe",
	component: Empty,
	tags: ["coverage-ui"],
	parameters: {
		layout: "fullscreen",
		mockApi: {
			data: {
				nodes,
				endpoints,
			},
			probe: {
				historyByEndpointId: {
					[ENDPOINT_ID]: history,
				},
				runsByRunId: {
					[fixtureCatalog.identifier.probeRunPrimary()]: run,
				},
			},
		},
	},
} satisfies Meta<typeof Empty>;

export default meta;

type Story = StoryObj<typeof meta>;

export const HistoryWithNodeNames: Story = {
	render: () => <></>,
	parameters: {
		router: { initialEntry: `/endpoints/${ENDPOINT_ID}/probe` },
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const links = await canvas.findAllByRole("link", {
			name: /Open node details:/i,
		});
		expect(links.map((link) => link.textContent)).toEqual([
			fixtureCatalog.slotString.s207(),
			fixtureCatalog.slotString.s210(),
			fixtureCatalog.slotString.s99(),
		]);
	},
};

export const LiveRunWithNodeNames: Story = {
	render: () => <></>,
	parameters: {
		router: {
			initialEntry: `/endpoints/probe/runs/${fixtureCatalog.identifier.probeRunPrimary()}`,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const runners = await canvas.findByText("Node runners (progress)");
		runners.click();
		const links = await canvas.findAllByRole("link", {
			name: /Open node details:/i,
		});
		expect(links.map((link) => link.textContent)).toEqual([
			fixtureCatalog.slotString.s207(),
			fixtureCatalog.slotString.s210(),
			fixtureCatalog.slotString.s99(),
		]);
	},
};
