import type { Meta, StoryObj } from "@storybook/react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { MeshUptimeStrip } from "./MeshUptimeStrip";

function buckets(values: Array<"good" | "fallback" | "slow" | "down">) {
	return values.map((value) => ({
		minute: fixtureCatalog.timestamp.t20260803T000000(),
		mesh_success: value === "good" || value === "slow" ? 1 : 0,
		mesh_failure: value === "down" || value === "fallback" ? 1 : 0,
		public_success: value === "fallback" ? 1 : 0,
		public_failure: 0,
		fallback_success: value === "fallback" ? 1 : 0,
		end_to_end_success: value === "down" ? 0 : 1,
		end_to_end_failure: value === "down" ? 1 : 0,
		latency_samples_ms: [value === "slow" ? 640 : 32],
		mesh_h2_requests: value === "good" || value === "slow" ? 1 : 0,
		mesh_connection_starts: 0,
	}));
}

const meta = {
	title: "Components/MeshUptimeStrip",
	component: MeshUptimeStrip,
	tags: ["autodocs", "coverage-ui"],
	parameters: { layout: "padded" },
} satisfies Meta<typeof MeshUptimeStrip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Healthy: Story = {
	args: {
		buckets: buckets(["good", "good", "good", "good"]),
		quality: "good",
		label: "24 hour availability: good",
	},
};

export const PublicFallback: Story = {
	args: {
		buckets: buckets(["good", "fallback", "fallback", "good"]),
		quality: "good",
		label: "24 hour availability with public fallback",
	},
};

export const Degraded: Story = {
	args: {
		buckets: buckets(["good", "slow", "down", "good"]),
		quality: "unstable",
		label: "24 hour availability: unstable",
	},
};

export const Empty: Story = {
	args: { buckets: [], quality: "unknown", label: "no mesh samples" },
};
