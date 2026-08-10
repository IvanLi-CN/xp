import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type {
	AdminEndpointProbeHistoryResponse,
	AdminEndpointProbeRunStatusResponse,
} from "../api/adminEndpointProbes";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";

const ENDPOINT_ID = fixtureCatalog.endpointId.fixture120();

const nodes: AdminNode[] = [
	{
		node_id: fixtureCatalog.nodeId.fixture206(),
		node_name: fixtureCatalog.nodeName.fixture207(),
		api_base_url: fixtureCatalog.service.fixture208(),
		access_host: fixtureCatalog.host.fixture209(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: fixtureCatalog.nodeId.fixture134(),
		node_name: fixtureCatalog.nodeName.fixture210(),
		api_base_url: fixtureCatalog.service.fixture211(),
		access_host: fixtureCatalog.host.fixture212(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: fixtureCatalog.nodeId.fixture213(),
		node_name: fixtureCatalog.host.fixture99(),
		api_base_url: fixtureCatalog.service.fixture214(),
		access_host: fixtureCatalog.host.fixture215(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
];

const endpoints: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.endpointId.fixture120(),
		node_id: fixtureCatalog.nodeId.fixture134(),
		tag: fixtureCatalog.endpointTag.fixture216(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: 443,
		meta: {},
		probe: {
			latest_checked_at: fixtureCatalog.timestamp.t20260728T140015(),
			latest_latency_ms_p50: 32,
			slots: [
				{
					hour: fixtureCatalog.timestamp.probeHour(),
					status: "up",
					checked_at: fixtureCatalog.timestamp.t20260728T140015(),
					latency_ms_p50: 32,
				},
			],
		},
	},
];

const history: AdminEndpointProbeHistoryResponse = {
	endpoint_id: fixtureCatalog.endpointId.fixture120(),
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
					node_id: fixtureCatalog.nodeId.fixture134(),
					ok: true,
					checked_at: fixtureCatalog.timestamp.t20260728T140010(),
					latency_ms: fixtureCatalog.number.value32(),
					config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				},
				{
					node_id: fixtureCatalog.nodeId.fixture206(),
					ok: true,
					checked_at: fixtureCatalog.timestamp.t20260728T140012(),
					latency_ms: fixtureCatalog.number.value76(),
					config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				},
				{
					node_id: fixtureCatalog.nodeId.fixture213(),
					ok: true,
					checked_at: fixtureCatalog.timestamp.t20260728T140015(),
					latency_ms: fixtureCatalog.number.value28(),
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
			node_id: fixtureCatalog.nodeId.fixture134(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.timestamp.t20240101T034100(),
				finished_at: fixtureCatalog.timestamp.t20260728T140010(),
			},
		},
		{
			node_id: fixtureCatalog.nodeId.fixture206(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.timestamp.t20240101T034100(),
				finished_at: fixtureCatalog.timestamp.t20260728T140012(),
			},
		},
		{
			node_id: fixtureCatalog.nodeId.fixture213(),
			status: "finished",
			progress: {
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				hour: fixtureCatalog.timestamp.probeHour(),
				config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
				status: "finished",
				endpoints_total: 1,
				endpoints_done: 1,
				started_at: fixtureCatalog.timestamp.baseline(),
				updated_at: fixtureCatalog.timestamp.t20240101T034100(),
				finished_at: fixtureCatalog.timestamp.t20260728T140015(),
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
				historyByEndpointId: Object.fromEntries([[ENDPOINT_ID, history]]),
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
			fixtureCatalog.nodeName.fixture207(),
			fixtureCatalog.nodeName.fixture210(),
			fixtureCatalog.host.fixture99(),
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
			fixtureCatalog.nodeName.fixture207(),
			fixtureCatalog.nodeName.fixture210(),
			fixtureCatalog.host.fixture99(),
		]);
	},
};
