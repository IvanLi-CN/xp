import type { Meta, StoryObj } from "@storybook/react";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

import type { AdminEndpointProbeSlot } from "../api/adminEndpoints";
import { EndpointProbeBar } from "./EndpointProbeBar";

function createSlots(
	statuses: AdminEndpointProbeSlot["status"][],
): AdminEndpointProbeSlot[] {
	return statuses.map((status, index) => ({
		hour: `${String(index).padStart(2, "0")}:00`,
		status,
		checked_at: `2026-03-09T${String(index).padStart(2, "0")}:00:00Z`,
		latency_ms_p50:
			status === "missing" || status === "down" ? undefined : 40 + index * 3,
	}));
}

function EndpointProbeBarStory({ slots }: { slots: AdminEndpointProbeSlot[] }) {
	return (
		<Card className="w-[360px]">
			<CardHeader className="pb-3">
				<CardTitle className="text-sm">Last 12 hours</CardTitle>
			</CardHeader>
			<CardContent className="space-y-3 pt-0">
				<EndpointProbeBar slots={slots} className="gap-1" />
				<div className="flex flex-wrap gap-2 text-xs">
					<Badge variant="success" size="sm">
						up
					</Badge>
					<Badge variant="warning" size="sm">
						degraded
					</Badge>
					<Badge variant="destructive" size="sm">
						down
					</Badge>
					<Badge variant="outline" size="sm">
						missing
					</Badge>
				</div>
			</CardContent>
		</Card>
	);
}

const meta = {
	title: "Components/EndpointProbeBar",
	component: EndpointProbeBarStory,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Compact probe-health spark bar used in endpoint lists and details. Stories cover the normal mixed-status run, gaps from missing telemetry, and the empty placeholder state. Theme changes come from the current semantic tokens; density does not change the probe block geometry.",
			},
		},
	},
	args: {
		slots: createSlots([
			"up",
			"up",
			"up",
			"degraded",
			"up",
			"down",
			"missing",
			"up",
			"up",
			"degraded",
			"up",
			"up",
		]),
	},
} satisfies Meta<typeof EndpointProbeBarStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithGaps: Story = {
	args: {
		slots: createSlots([
			"missing",
			"missing",
			"up",
			"degraded",
			"down",
			"down",
			"missing",
			"up",
			"degraded",
			"up",
			"missing",
			"up",
		]),
	},
};

export const Empty: Story = {
	args: {
		slots: [],
	},
};
