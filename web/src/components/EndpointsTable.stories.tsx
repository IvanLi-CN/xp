import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import type { ReactNode } from "react";

import type {
	AdminEndpoint,
	AdminEndpointProbeSlot,
} from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { EndpointsTable } from "./EndpointsTable";

function makeSlots(): AdminEndpointProbeSlot[] {
	const slots: AdminEndpointProbeSlot[] = [];
	for (let hour = 0; hour < 24; hour++) {
		const hh = String(hour).padStart(2, "0");
		slots.push({
			hour: `2026-02-19T${hh}:00:00Z`,
			status: hour % 11 === 0 ? "down" : hour % 7 === 0 ? "degraded" : "up",
			latency_ms_p50: 200 + hour,
			checked_at: `2026-02-19T${hh}:00:10Z`,
		});
	}
	return slots;
}

const LONG_NODE_ID = "01KGRVRYQS9VA9JFEPO0NR6MD2B";
const LONG_ENDPOINT_ID =
	"ep_01HENDPT_THIS_IS_A_VERY_LONG_ENDPOINT_ID_FOR_LAYOUT_TESTING";
const LONG_TAG =
	"edge-tokyo-with-a-very-long-tag-that-should-truncate-nicely-in-the-table";
const LONG_NODE_NAME =
	"tokyo-edge-with-a-very-long-node-name-that-should-truncate-nicely";

const ENDPOINTS: AdminEndpoint[] = [
	{
		endpoint_id: LONG_ENDPOINT_ID,
		node_id: LONG_NODE_ID,
		tag: LONG_TAG,
		kind: "vless_reality_vision_tcp",
		port: 53842,
		meta: { public_domain: "tokyo.example.invalid" },
		probe: {
			latest_checked_at: "2026-02-19T23:00:10Z",
			latest_latency_ms_p50: 293,
			slots: makeSlots(),
		},
	},
	{
		endpoint_id: "ep_01HENDPT_SHORT",
		node_id: "01KFTEA58X1RXXVDRD6EPFB63Y",
		tag: "osaka-ss2022",
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 53843,
		meta: { public_domain: "osaka.example.invalid" },
		probe: {
			latest_checked_at: "2026-02-19T23:00:10Z",
			latest_latency_ms_p50: 223,
			slots: makeSlots(),
		},
	},
];

const NODES: AdminNode[] = [
	{
		node_id: LONG_NODE_ID,
		node_name: LONG_NODE_NAME,
		api_base_url: "https://tokyo.example.invalid",
		access_host: "tokyo.example.invalid",
		quota_reset: { policy: "unlimited" },
	},
	{
		node_id: "01KFTEA58X1RXXVDRD6EPFB63Y",
		node_name: "osaka-1",
		api_base_url: "https://osaka.example.invalid",
		access_host: "osaka.example.invalid",
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
		const tags = await canvas.findAllByText(/edge-tokyo-with-a-very-long-tag/);
		expect(tags).toHaveLength(2);

		const vless = await canvas.findAllByText("VLESS");
		expect(vless).toHaveLength(2);

		const ss2022 = await canvas.findAllByText("SS2022");
		expect(ss2022).toHaveLength(2);

		const nodeNames = await canvas.findAllByText(LONG_NODE_NAME);
		expect(nodeNames).toHaveLength(2);
	},
};
