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
			hour: fixtureCatalog.slotString.s4(),
			status: "down",
			latency_ms_p50: 200,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s5(),
			status: "up",
			latency_ms_p50: 201,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s6(),
			status: "up",
			latency_ms_p50: 202,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s7(),
			status: "up",
			latency_ms_p50: 203,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s8(),
			status: "up",
			latency_ms_p50: 204,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s9(),
			status: "up",
			latency_ms_p50: 205,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s10(),
			status: "up",
			latency_ms_p50: 206,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s11(),
			status: "degraded",
			latency_ms_p50: 207,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s12(),
			status: "up",
			latency_ms_p50: 208,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s13(),
			status: "up",
			latency_ms_p50: 209,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s14(),
			status: "up",
			latency_ms_p50: 210,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s15(),
			status: "down",
			latency_ms_p50: 211,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s16(),
			status: "up",
			latency_ms_p50: 212,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s21(),
			status: "up",
			latency_ms_p50: 213,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s22(),
			status: "degraded",
			latency_ms_p50: 214,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s23(),
			status: "up",
			latency_ms_p50: 215,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s24(),
			status: "up",
			latency_ms_p50: 216,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s25(),
			status: "up",
			latency_ms_p50: 217,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s26(),
			status: "up",
			latency_ms_p50: 218,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s47(),
			status: "up",
			latency_ms_p50: 219,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s48(),
			status: "up",
			latency_ms_p50: 220,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s52(),
			status: "degraded",
			latency_ms_p50: 221,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s54(),
			status: "down",
			latency_ms_p50: 222,
			checked_at: fixtureCatalog.slotString.s269(),
		},
		{
			hour: fixtureCatalog.slotString.s81(),
			status: "up",
			latency_ms_p50: 223,
			checked_at: fixtureCatalog.slotString.s269(),
		},
	];
}

const LONG_NODE_NAME = fixtureCatalog.slotString.s276();

const ENDPOINTS: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.slotString.s270(),
		node_id: fixtureCatalog.slotString.s271(),
		tag: fixtureCatalog.slotString.s272(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: 53842,
		meta: { public_domain: fixtureCatalog.slotString.s212() },
		probe: {
			latest_checked_at: fixtureCatalog.timestamp.probeLatest(),
			latest_latency_ms_p50: 293,
			slots: makeSlots(),
		},
	},
	{
		endpoint_id: fixtureCatalog.slotString.s273(),
		node_id: fixtureCatalog.slotString.s274(),
		tag: fixtureCatalog.slotString.s275(),
		kind: fixtureCatalog.endpoint.ssKind(),
		port: 53843,
		meta: { public_domain: fixtureCatalog.slotString.s278() },
		probe: {
			latest_checked_at: fixtureCatalog.timestamp.probeLatest(),
			latest_latency_ms_p50: 223,
			slots: makeSlots(),
		},
	},
];

const NODES: AdminNode[] = [
	{
		node_id: fixtureCatalog.slotString.s271(),
		node_name: fixtureCatalog.slotString.s276(),
		api_base_url: fixtureCatalog.slotString.s211(),
		access_host: fixtureCatalog.slotString.s212(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: fixtureCatalog.slotString.s274(),
		node_name: fixtureCatalog.slotString.s37(),
		api_base_url: fixtureCatalog.slotString.s277(),
		access_host: fixtureCatalog.slotString.s278(),
		quota_limit_bytes: 0,
		quota_reset: { policy: "unlimited" },
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
		const tags = await canvas.findAllByText(fixtureCatalog.slotString.s272());
		expect(tags).toHaveLength(2);

		const vless = await canvas.findAllByText("VLESS");
		expect(vless).toHaveLength(2);

		const ss2022 = await canvas.findAllByText("SS2022");
		expect(ss2022).toHaveLength(2);

		const nodeNames = await canvas.findAllByText(LONG_NODE_NAME);
		expect(nodeNames).toHaveLength(2);
	},
};
