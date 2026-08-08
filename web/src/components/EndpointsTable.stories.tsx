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
	const slots: AdminEndpointProbeSlot[] = [];
	for (let hour = 0; hour < 24; hour++) {
		const hh = String(hour).padStart(2, "0");
		slots.push({
			hour: `2026-02-19T${hh}:00:00Z`,
			status: hour % 11 === 0 ? "down" : hour % 7 === 0 ? "degraded" : "up",
			latency_ms_p50: 200 + hour,
			checked_at: fixtureCatalog.slotString.s269(),
		});
	}
	return slots;
}

const LONG_NODE_NAME = fixtureCatalog.slotString.s276();

const ENDPOINTS: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.slotString.s270(),
		node_id: fixtureCatalog.slotString.s271(),
		tag: fixtureCatalog.slotString.s272(),
		kind: "vless_reality_vision_tcp",
		port: 53842,
		meta: { public_domain: fixtureCatalog.slotString.s212() },
		probe: {
			latest_checked_at: "2026-02-19T23:00:10Z",
			latest_latency_ms_p50: 293,
			slots: makeSlots(),
		},
	},
	{
		endpoint_id: fixtureCatalog.slotString.s273(),
		node_id: fixtureCatalog.slotString.s274(),
		tag: fixtureCatalog.slotString.s275(),
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 53843,
		meta: { public_domain: fixtureCatalog.slotString.s278() },
		probe: {
			latest_checked_at: "2026-02-19T23:00:10Z",
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
