import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const NODE_ID = "node-1";
const ENDPOINT_ID = "endpoint-managed-vless";

const meta = {
	title: "Pages/EndpointDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/endpoints/${ENDPOINT_ID}`,
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "tokyo-1",
						access_host: "tavily-tw.ivanli.cc",
						api_base_url: "https://tokyo-1.example.com",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				endpoints: [
					{
						endpoint_id: ENDPOINT_ID,
						node_id: NODE_ID,
						tag: "managed-vless",
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["tavily-tw.ivanli.cc"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080",
								mode: "auto",
							},
							accepted_authorities: [
								"hinet-ep.707979.xyz:53844",
								"edge.example.com:53844",
							],
						},
						short_ids: ["2a3b4c"],
						active_short_id: "2a3b4c",
					},
				],
				realityDomains: [
					{
						domain_id: "seed-public-sn-files",
						server_name: "public.sn.files.1drv.com",
						disabled_node_ids: [],
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManagedDefaultAliases: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "Endpoint details" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("acceptedAuthorities"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("hinet-ep.707979.xyz:53844"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("edge.example.com:53844"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				"Accept additional ordinary HTTPS Host headers for camouflage routing. This does not change REALITY serverNames or the canonical /generate_204 URL.",
			),
		).toBeInTheDocument();
	},
};
