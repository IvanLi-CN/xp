import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import type { ReactNode } from "react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type {
	AdminEndpoint,
	AdminEndpointProbeSlot,
} from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { EndpointsTable } from "./EndpointsTable";

function makeSlots(): AdminEndpointProbeSlot[] {
	return [
		{
			hour: fixtureCatalog.timestamp.t20240101T000400(),
			status: "down",
			latency_ms_p50: 200,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000500(),
			status: "up",
			latency_ms_p50: 201,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000600(),
			status: "up",
			latency_ms_p50: 202,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000700(),
			status: "up",
			latency_ms_p50: 203,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000800(),
			status: "up",
			latency_ms_p50: 204,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000900(),
			status: "up",
			latency_ms_p50: 205,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001000(),
			status: "up",
			latency_ms_p50: 206,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001100(),
			status: "degraded",
			latency_ms_p50: 207,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001200(),
			status: "up",
			latency_ms_p50: 208,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001300(),
			status: "up",
			latency_ms_p50: 209,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001400(),
			status: "up",
			latency_ms_p50: 210,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001500(),
			status: "down",
			latency_ms_p50: 211,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001600(),
			status: "up",
			latency_ms_p50: 212,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T002100(),
			status: "up",
			latency_ms_p50: 213,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T002200(),
			status: "degraded",
			latency_ms_p50: 214,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T002300(),
			status: "up",
			latency_ms_p50: 215,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260308T000000(),
			status: "up",
			latency_ms_p50: 216,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260308T000200(),
			status: "up",
			latency_ms_p50: 217,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260308T000100(),
			status: "up",
			latency_ms_p50: 218,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260307T010000(),
			status: "up",
			latency_ms_p50: 219,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260308T005900(),
			status: "up",
			latency_ms_p50: 220,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260308T005800(),
			status: "degraded",
			latency_ms_p50: 221,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20260131T000000(),
			status: "down",
			latency_ms_p50: 222,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T012100(),
			status: "up",
			latency_ms_p50: 223,
			checked_at: fixtureCatalog.timestamp.t20240101T042900(),
		},
	];
}

const LONG_NODE_NAME = fixtureCatalog.nodeName.fixture276();

const ENDPOINTS: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.endpointId.fixture270(),
		node_id: fixtureCatalog.nodeId.fixture271(),
		tag: fixtureCatalog.endpointTag.fixture272(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port53842(),
		meta: { public_domain: fixtureCatalog.host.fixture212() },
		probe: {
			latest_checked_at: fixtureCatalog.timestamp.probeLatest(),
			latest_latency_ms_p50: 293,
			slots: makeSlots(),
		},
	},
	{
		endpoint_id: fixtureCatalog.endpointId.fixture273(),
		node_id: fixtureCatalog.nodeId.fixture274(),
		tag: fixtureCatalog.endpointTag.fixture275(),
		kind: fixtureCatalog.endpoint.ssKind(),
		port: fixtureCatalog.endpoint.port53843(),
		meta: { public_domain: fixtureCatalog.host.fixture278() },
		probe: {
			latest_checked_at: fixtureCatalog.timestamp.probeLatest(),
			latest_latency_ms_p50: 223,
			slots: makeSlots(),
		},
	},
];

const NODES: AdminNode[] = [
	{
		node_id: fixtureCatalog.nodeId.fixture271(),
		node_name: fixtureCatalog.nodeName.fixture276(),
		api_base_url: fixtureCatalog.service.fixture211(),
		access_host: fixtureCatalog.host.fixture212(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset: fixtureCatalog.quota.resetUnlimited(),
	},
	{
		node_id: fixtureCatalog.nodeId.fixture274(),
		node_name: fixtureCatalog.nodeName.fixture37(),
		api_base_url: fixtureCatalog.service.fixture277(),
		access_host: fixtureCatalog.host.fixture278(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset: fixtureCatalog.quota.resetUnlimited(),
	},
];

const NODE_BY_ID = new Map(NODES.map((n) => [n.node_id, n] as const));

function Frame(props: { width: number; label: string; children: ReactNode }) {
	const { width, label, children } = props;
	return (
		<div className="space-y-2">
			<div className="text-xs font-mono opacity-60">
				{label} ({width}px)
			</div>
			<div data-testid={`frame-${width}`} style={{ width }}>
				{children}
			</div>
		</div>
	);
}

const meta: Meta<typeof EndpointsTable> = {
	title: "Components/EndpointsTable",
	component: EndpointsTable,
	tags: ["autodocs", "coverage-ui"],
};

export default meta;

type Story = StoryObj<typeof EndpointsTable>;

export const ResponsiveNoScroll: Story = {
	render: () => (
		<div className="space-y-6">
			<Frame width={648} label="Target (>=1024px main content)">
				<EndpointsTable endpoints={ENDPOINTS} nodeById={NODE_BY_ID} />
			</Frame>
			<Frame width={936} label="Wide desktop">
				<EndpointsTable endpoints={ENDPOINTS} nodeById={NODE_BY_ID} />
			</Frame>
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		for (const width of [648, 936]) {
			const frame = canvas.getByTestId(`frame-${width}`);
			const scroller = frame.querySelector(".overflow-x-auto");
			expect(scroller).toBeTruthy();
			if (!scroller) continue;
			expect(scroller.scrollWidth).toBeLessThanOrEqual(scroller.clientWidth);
		}

		// Sanity-check key fields are rendered (CSS truncation doesn't change textContent).
		const tags = await canvas.findAllByText(
			fixtureCatalog.endpointTag.fixture272(),
		);
		expect(tags).toHaveLength(2);

		const vless = await canvas.findAllByText("VLESS");
		expect(vless).toHaveLength(2);

		const ss2022 = await canvas.findAllByText("SS2022");
		expect(ss2022).toHaveLength(2);

		const nodeNames = await canvas.findAllByText(LONG_NODE_NAME);
		expect(nodeNames).toHaveLength(2);
	},
};
