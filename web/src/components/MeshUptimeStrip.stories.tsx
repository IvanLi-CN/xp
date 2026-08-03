import type { Meta, StoryObj } from "@storybook/react";

import { MeshUptimeStrip } from "./MeshUptimeStrip";

function buckets(values: Array<"good" | "fallback" | "slow" | "down">) {
	return values.map((value, index) => ({
		minute: `2026-08-03T00:${String(index).padStart(2, "0")}:00Z`,
		mesh_success: value === "good" || value === "slow" ? 1 : 0,
		mesh_failure: value === "down" || value === "fallback" ? 1 : 0,
		public_success: value === "fallback" ? 1 : 0,
		public_failure: 0,
		fallback_success: value === "fallback" ? 1 : 0,
		latency_samples_ms: [value === "slow" ? 640 : 32],
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
