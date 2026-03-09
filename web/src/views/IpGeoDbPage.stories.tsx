import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const sharedNodes = [
	{
		node_id: "node-tokyo",
		node_name: "Tokyo",
		api_base_url: "https://tokyo.example.com",
		access_host: "tokyo.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
	{
		node_id: "node-osaka",
		node_name: "Osaka",
		api_base_url: "https://osaka.example.com",
		access_host: "osaka.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
	{
		node_id: "node-seoul",
		node_name: "Seoul",
		api_base_url: "https://seoul.example.com",
		access_host: "seoul.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
] as const;

const meta = {
	title: "Pages/IpGeoDbPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/ip-geo-db",
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManagedHealthy: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [...sharedNodes],
				ipGeoDb: {
					settings: {
						provider: "dbip_lite",
						auto_update_enabled: true,
						update_interval_days: 7,
					},
					partial: false,
					unreachable_nodes: [],
					nodes: [
						{
							node: sharedNodes[0],
							mode: "managed",
							running: false,
							city_db_path: "/var/lib/xp/geoip/dbip-city-lite.mmdb",
							asn_db_path: "/var/lib/xp/geoip/dbip-asn-lite.mmdb",
							last_started_at: "2026-03-08T00:00:00Z",
							last_success_at: "2026-03-08T00:01:30Z",
							next_scheduled_at: "2026-03-15T00:01:30Z",
							last_error: null,
						},
						{
							node: sharedNodes[1],
							mode: "managed",
							running: false,
							city_db_path: "/var/lib/xp/geoip/dbip-city-lite.mmdb",
							asn_db_path: "/var/lib/xp/geoip/dbip-asn-lite.mmdb",
							last_started_at: "2026-03-08T00:00:00Z",
							last_success_at: "2026-03-08T00:01:40Z",
							next_scheduled_at: "2026-03-15T00:01:40Z",
							last_error: null,
						},
					],
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "IP geolocation" }),
		).toBeInTheDocument();
		await expect(await canvas.findByText("Update policy")).toBeInTheDocument();
		const managedBadges = await canvas.findAllByText("Managed");
		await expect(managedBadges.length).toBeGreaterThan(0);
	},
};

export const RunningPartialExternalOverride: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [...sharedNodes],
				ipGeoDb: {
					settings: {
						provider: "dbip_lite",
						auto_update_enabled: false,
						update_interval_days: 1,
					},
					partial: true,
					unreachable_nodes: ["node-seoul"],
					nodes: [
						{
							node: sharedNodes[0],
							mode: "managed",
							running: true,
							city_db_path: "/var/lib/xp/geoip/dbip-city-lite.mmdb",
							asn_db_path: "/var/lib/xp/geoip/dbip-asn-lite.mmdb",
							last_started_at: "2026-03-09T08:00:00Z",
							last_success_at: "2026-03-08T08:00:00Z",
							next_scheduled_at: null,
							last_error: null,
						},
						{
							node: sharedNodes[1],
							mode: "external_override",
							running: false,
							city_db_path: "/etc/xp/custom-city.mmdb",
							asn_db_path: "/etc/xp/custom-asn.mmdb",
							last_started_at: null,
							last_success_at: null,
							next_scheduled_at: null,
							last_error:
								"Managed downloader is skipped because env override is present.",
						},
						{
							node: sharedNodes[2],
							mode: "missing",
							running: false,
							city_db_path: "",
							asn_db_path: "",
							last_started_at: null,
							last_success_at: null,
							next_scheduled_at: null,
							last_error: "download failed: upstream timeout",
						},
					],
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("Node status is partial."),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("External override"),
		).toBeInTheDocument();
		await expect(await canvas.findByText("running")).toBeInTheDocument();
	},
};
