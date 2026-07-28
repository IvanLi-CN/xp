import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

import type {
	AdminEndpointProbeHistoryResponse,
	AdminEndpointProbeRunStatusResponse,
} from "../api/adminEndpointProbes";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";

const ENDPOINT_ID = "endpoint-probe-demo";
const RUN_ID = "run-probe-demo";
const HOUR = "2026-07-28T14:00:00Z";
const CONFIG_HASH = "config-demo-20260728";

const nodes: AdminNode[] = [
	{
		node_id: "node-amsterdam",
		node_name: "Amsterdam edge",
		api_base_url: "https://amsterdam.example.invalid",
		access_host: "amsterdam.example.invalid",
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: "node-tokyo",
		node_name: "Tokyo edge",
		api_base_url: "https://tokyo.example.invalid",
		access_host: "tokyo.example.invalid",
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: "node-retired",
		node_name: "",
		api_base_url: "https://retired.example.invalid",
		access_host: "retired.example.invalid",
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
];

const endpoints: AdminEndpoint[] = [
	{
		endpoint_id: ENDPOINT_ID,
		node_id: "node-tokyo",
		tag: "vless-vision-tokyo",
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {},
		probe: {
			latest_checked_at: "2026-07-28T14:00:15Z",
			latest_latency_ms_p50: 32,
			slots: [
				{
					hour: HOUR,
					status: "up",
					checked_at: "2026-07-28T14:00:15Z",
					latency_ms_p50: 32,
				},
			],
		},
	},
];

const history: AdminEndpointProbeHistoryResponse = {
	endpoint_id: ENDPOINT_ID,
	participating_nodes: 3,
	slots: [
		{
			hour: HOUR,
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
					node_id: "node-tokyo",
					ok: true,
					checked_at: "2026-07-28T14:00:10Z",
					latency_ms: 32,
					config_hash: CONFIG_HASH,
				},
				{
					node_id: "node-amsterdam",
					ok: true,
					checked_at: "2026-07-28T14:00:12Z",
					latency_ms: 76,
					config_hash: CONFIG_HASH,
				},
				{
					node_id: "node-retired",
					ok: true,
					checked_at: "2026-07-28T14:00:15Z",
					latency_ms: 28,
					config_hash: CONFIG_HASH,
				},
			],
		},
	],
};

const run: AdminEndpointProbeRunStatusResponse = {
	run_id: RUN_ID,
	status: "finished",
	hour: HOUR,
	config_hash: CONFIG_HASH,
	nodes: history.slots[0].by_node.map((sample) => ({
		node_id: sample.node_id,
		status: "finished",
		progress: {
			run_id: RUN_ID,
			hour: HOUR,
			config_hash: CONFIG_HASH,
			status: "finished",
			endpoints_total: 1,
			endpoints_done: 1,
			started_at: "2026-07-28T14:00:00Z",
			updated_at: sample.checked_at,
			finished_at: sample.checked_at,
		},
	})),
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
					[RUN_ID]: run,
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
			"Amsterdam edge",
			"Tokyo edge",
		]);
		expect(await canvas.findByText("node-retired")).toBeInTheDocument();
	},
};

export const LiveRunWithNodeNames: Story = {
	render: () => <></>,
	parameters: {
		router: { initialEntry: `/endpoints/probe/runs/${RUN_ID}` },
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const runners = await canvas.findByText("Node runners (progress)");
		runners.click();
		const links = await canvas.findAllByRole("link", {
			name: /Open node details:/i,
		});
		expect(links.map((link) => link.textContent)).toEqual([
			"Amsterdam edge",
			"Tokyo edge",
		]);
		expect(await canvas.findByText("node-retired")).toBeInTheDocument();
	},
};
