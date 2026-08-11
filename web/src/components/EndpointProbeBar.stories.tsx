import type { Meta, StoryObj } from "@storybook/react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

import type { AdminEndpointProbeSlot } from "../api/adminEndpoints";
import { EndpointProbeBar } from "./EndpointProbeBar";

function createSlots(
	statuses: AdminEndpointProbeSlot["status"][],
): AdminEndpointProbeSlot[] {
	const [
		slot0,
		slot1,
		slot2,
		slot3,
		slot4,
		slot5,
		slot6,
		slot7,
		slot8,
		slot9,
		slot10,
		slot11,
	] = statuses;
	return [
		{
			hour: fixtureCatalog.timestamp.t20240101T000400(),
			status: slot0,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot0 === "missing" || slot0 === "down" ? undefined : 40,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000500(),
			status: slot1,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot1 === "missing" || slot1 === "down" ? undefined : 43,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000600(),
			status: slot2,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot2 === "missing" || slot2 === "down" ? undefined : 46,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000700(),
			status: slot3,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot3 === "missing" || slot3 === "down" ? undefined : 49,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000800(),
			status: slot4,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot4 === "missing" || slot4 === "down" ? undefined : 52,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T000900(),
			status: slot5,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot5 === "missing" || slot5 === "down" ? undefined : 55,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001000(),
			status: slot6,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot6 === "missing" || slot6 === "down" ? undefined : 58,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001100(),
			status: slot7,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot7 === "missing" || slot7 === "down" ? undefined : 61,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001200(),
			status: slot8,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot8 === "missing" || slot8 === "down" ? undefined : 64,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001300(),
			status: slot9,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50: slot9 === "missing" || slot9 === "down" ? undefined : 67,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001400(),
			status: slot10,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50:
				slot10 === "missing" || slot10 === "down" ? undefined : 70,
		},
		{
			hour: fixtureCatalog.timestamp.t20240101T001500(),
			status: slot11,
			checked_at: fixtureCatalog.timestamp.t20240101T042800(),
			latency_ms_p50:
				slot11 === "missing" || slot11 === "down" ? undefined : 73,
		},
	];
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
