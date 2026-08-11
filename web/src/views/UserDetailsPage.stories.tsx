import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	buildDenseUserIpUsageStories,
	buildDuplicateNameUserIpUsageStories,
} from "../storybook/ipUsageStoryData";

const USER_ID_1 = fixtureCatalog.identifier.userPrimary();
const USER_ID_2 = fixtureCatalog.identifier.userSecondary();
const ACCESS_NODE_HINT = [
	"Node all-select covers current endpoints on",
	fixtureCatalog.identifier.nodeNameSecondary(),
	"and",
	fixtureCatalog.identifier.nodeNamePrimary(),
	"only. Future endpoints still follow protocol all-select defaults.",
].join(" ");

const nodes: AdminNode[] = [
	{
		node_id: fixtureCatalog.identifier.nodePrimary(),
		node_name: fixtureCatalog.identifier.nodeNameSecondary(),
		access_host: fixtureCatalog.host.primary(),
		api_base_url: fixtureCatalog.url.primaryApi(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset: fixtureCatalog.quota.resetNode(),
	},
	{
		node_id: fixtureCatalog.identifier.nodeSecondary(),
		node_name: fixtureCatalog.identifier.nodeNamePrimary(),
		access_host: fixtureCatalog.host.secondary(),
		api_base_url: fixtureCatalog.url.secondaryApi(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset: fixtureCatalog.quota.resetNode(),
	},
];

const endpoints: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		tag: fixtureCatalog.identifier.endpointTagPrimary(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
		meta: {},
	},
	{
		endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
		node_id: fixtureCatalog.identifier.nodeSecondary(),
		tag: fixtureCatalog.identifier.endpointTagSecondary(),
		kind: fixtureCatalog.endpoint.ssKind(),
		port: fixtureCatalog.endpoint.port8443(),
		meta: {},
	},
];

const userUsageReports = buildDenseUserIpUsageStories();

const meta = {
	title: "Pages/UserDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/users/${USER_ID_1}`,
		},
		mockApi: {
			data: {
				endpoints,
				nodes,
				userAccessByUserId: Object.fromEntries([
					[
						USER_ID_1,
						[
							{
								user_id: fixtureCatalog.identifier.userPrimary(),
								endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
								node_id: fixtureCatalog.identifier.nodePrimary(),
							},
							{
								user_id: fixtureCatalog.identifier.userPrimary(),
								endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
								node_id: fixtureCatalog.identifier.nodeSecondary(),
							},
						],
					],
				]),
				nodeQuotas: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						node_id: fixtureCatalog.identifier.nodePrimary(),
						quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
						quota_reset_source: "user",
					},
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						node_id: fixtureCatalog.identifier.nodeSecondary(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
						quota_reset_source: "node",
					},
				],
				userIpUsageByUserId: Object.fromEntries([
					[USER_ID_1, userUsageReports],
				]),
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const User1: Story = {};

export const UserAndMihomoSections: Story = {
	tags: ["user-mihomo-layout"],
	beforeEach: () => {
		const originalFetch = globalThis.fetch;
		globalThis.fetch = async (input, init) => {
			const request = new Request(
				typeof input === "string"
					? new URL(input, window.location.href)
					: input,
				init,
			);
			if (
				new URL(request.url).pathname.endsWith("/subscription-mihomo-profile")
			) {
				return new Response(
					JSON.stringify({
						mixin_yaml: "port: 0\nproxy-groups: []\n",
						extra_proxies_yaml: "",
						extra_proxy_providers_yaml: "",
					}),
					{ headers: { "Content-Type": "application/json" } },
				);
			}
			return originalFetch(request);
		};
		return () => {
			globalThis.fetch = originalFetch;
		};
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const userProfile = await canvas.findByRole("region", {
			name: "User profile",
		});
		const mihomoConfig = await canvas.findByRole("region", {
			name: "Mihomo configuration",
		});

		await expect(
			within(userProfile).getByRole("button", { name: "Save user" }),
		).toBeInTheDocument();
		await expect(
			within(userProfile).queryByRole("button", {
				name: "Save mihomo mixin",
			}),
		).not.toBeInTheDocument();
		await expect(
			within(mihomoConfig).getByRole("button", {
				name: "Save mihomo mixin",
			}),
		).toBeInTheDocument();
		await expect(
			within(mihomoConfig).queryByRole("button", { name: "Save user" }),
		).not.toBeInTheDocument();
	},
};

export const User2: Story = {
	parameters: {
		router: {
			initialEntry: `/users/${USER_ID_2}`,
		},
	},
};

export const AccessTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Access" }),
		);
		await expect(
			await canvas.findByText("Selected endpoints: 2", {}, { timeout: 5_000 }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				"After Apply access, new VLESS and SS2022 endpoints will be assigned to this user automatically.",
			),
		).toBeInTheDocument();
		await expect(await canvas.findByText(ACCESS_NODE_HINT)).toBeInTheDocument();
	},
};

export const QuotaStatusTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Quota status" }),
		);
		await expect(
			(await canvas.findAllByText(/Remaining:/)).length,
		).toBeGreaterThan(0);
	},
};

export const TrafficTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Traffic" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Traffic" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("combobox", { name: "Traffic nodes" }),
		).toHaveTextContent("All nodes");
		await userEvent.click(
			await canvas.findByRole("button", { name: "Last 31 days" }),
		);
	},
};

export const UsageDetailsTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await expect(
			await canvas.findByRole("tab", {
				name: fixtureCatalog.identifier.nodeNamePrimary(),
			}),
		).toHaveAttribute("aria-selected", "true");
		await expect(
			await canvas.findByText(
				`Usage details · ${fixtureCatalog.identifier.nodeNamePrimary()}`,
			),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", {
				name: fixtureCatalog.address.secondaryIpv4(),
			}),
		).toBeInTheDocument();
	},
};

export const UsageDetailsDuplicateNames: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [
					{
						node_id: fixtureCatalog.identifier.endpointPrimary(),
						node_name: fixtureCatalog.identifier.nodeNamePrimary(),
						access_host: fixtureCatalog.host.primary(),
						api_base_url: fixtureCatalog.url.primaryApi(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
					{
						node_id: fixtureCatalog.identifier.endpointSecondary(),
						node_name: fixtureCatalog.identifier.nodeNamePrimary(),
						access_host: fixtureCatalog.host.secondary(),
						api_base_url: fixtureCatalog.url.secondaryApi(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				userIpUsageByUserId: Object.fromEntries([
					[
						USER_ID_1,
						{
							...buildDuplicateNameUserIpUsageStories(),
						},
					],
				]),
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await expect(
			await canvas.findByRole("tab", {
				name: `fixture-duplicate · ${fixtureCatalog.host.primary()}`,
			}),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("tab", {
				name: `fixture-duplicate · ${fixtureCatalog.host.secondary()}`,
			}),
		).toBeInTheDocument();
	},
};

export const UsageDetailsTab7d: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await userEvent.click(
			await canvas.findByRole("tab", {
				name: fixtureCatalog.identifier.nodeNamePrimary(),
			}),
		);
		await expect(
			await canvas.findByRole("heading", {
				name: `Usage details · ${fixtureCatalog.identifier.nodeNamePrimary()}`,
			}),
		).toBeInTheDocument();
		await userEvent.click(await canvas.findByRole("button", { name: "7d" }));
		await expect(
			await canvas.findByRole("button", { name: "7d" }),
		).toHaveAttribute("aria-pressed", "true");
		await expect(
			await canvas.findByRole("button", {
				name: fixtureCatalog.address.secondaryIpv4(),
			}),
		).toBeInTheDocument();
	},
};

export const MihomoProviderPreview: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const subscriptionFormat = await canvas.findByTestId("subscription-format");
		subscriptionFormat.querySelectorAll("label")[2]?.click();
		await userEvent.click(await canvas.findByRole("button", { name: "Fetch" }));
		await expect(
			await screen.findByText("Subscription preview"),
		).toBeInTheDocument();
		await expect(
			await screen.findByText(fixtureCatalog.subscription.rawUri()),
		).toBeInTheDocument();
	},
};
