import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import type { ReactNode } from "react";

import type {
	AdminEndpoint,
	AdminEndpointProbeSlot,
} from "../api/adminEndpoints";
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
				<EndpointsTable endpoints={ENDPOINTS} />
			</Frame>
			<Frame width={936} label="Wide desktop">
				<EndpointsTable endpoints={ENDPOINTS} />
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
	},
};
