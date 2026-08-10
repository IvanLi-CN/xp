import type { Meta, StoryObj } from "@storybook/react";
import { fixtureCatalog } from "../fixture-policy/catalog";

const userIdA = fixtureCatalog.identifier.userPrimary();
const userIdB = fixtureCatalog.identifier.userSecondary();

const meta = {
	title: "Pages/QuotaPolicyPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/quota-policy",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: fixtureCatalog.slotString.s145(),
						node_name: fixtureCatalog.slotString.s146(),
						access_host: fixtureCatalog.slotString.s147(),
						api_base_url: fixtureCatalog.slotString.s148(),
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: fixtureCatalog.slotString.s149(),
						node_name: fixtureCatalog.slotString.s150(),
						access_host: fixtureCatalog.slotString.s151(),
						api_base_url: fixtureCatalog.slotString.s152(),
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: fixtureCatalog.slotString.s153(),
						node_name: fixtureCatalog.slotString.s154(),
						access_host: fixtureCatalog.slotString.s155(),
						api_base_url: fixtureCatalog.slotString.s156(),
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
						endpoint_id: fixtureCatalog.slotString.s157(),
						node_id: fixtureCatalog.slotString.s145(),
						tag: fixtureCatalog.slotString.s158(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: 443,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s159(),
								server_names: fixtureCatalog.slotList.l9(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.slotString.s160(),
						node_id: fixtureCatalog.slotString.s145(),
						tag: fixtureCatalog.slotString.s161(),
						kind: fixtureCatalog.endpoint.ssKind(),
						port: 8443,
						meta: {
							method: "2022-blake3-aes-128-gcm",
						},
					},
					{
						endpoint_id: fixtureCatalog.slotString.s162(),
						node_id: fixtureCatalog.slotString.s149(),
						tag: fixtureCatalog.slotString.s163(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: 444,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s164(),
								server_names: fixtureCatalog.slotList.l10(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.slotString.s165(),
						node_id: fixtureCatalog.slotString.s153(),
						tag: fixtureCatalog.slotString.s166(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: 445,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s167(),
								server_names: fixtureCatalog.slotList.l11(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
				],
				users: [
					{
						user_id: userIdA,
						display_name: "Ivan",
						subscription_token: fixtureCatalog.slotString.s168(),
						credential_epoch: 0,
						priority_tier: "p1",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
					{
						user_id: userIdB,
						display_name: "Koha",
						subscription_token: fixtureCatalog.slotString.s169(),
						credential_epoch: 0,
						priority_tier: "p2",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
				],
				userNodeWeights: Object.fromEntries([
					[
						userIdA,
						[
							{ node_id: fixtureCatalog.slotString.s145(), weight: 6500 },
							{ node_id: fixtureCatalog.slotString.s149(), weight: 5000 },
							{ node_id: fixtureCatalog.slotString.s153(), weight: 2000 },
						],
					],
					[
						userIdB,
						[
							{ node_id: fixtureCatalog.slotString.s145(), weight: 3500 },
							{ node_id: fixtureCatalog.slotString.s149(), weight: 5000 },
							{ node_id: fixtureCatalog.slotString.s153(), weight: 8000 },
						],
					],
				]),
				userGlobalWeights: Object.fromEntries([
					[userIdA, 6500],
					[userIdB, 3500],
				]),
				nodeWeightPolicies: {
					[fixtureCatalog.slotString.s145()]: {
						node_id: fixtureCatalog.slotString.s145(),
						inherit_global: false,
					},
					[fixtureCatalog.slotString.s149()]: {
						node_id: fixtureCatalog.slotString.s149(),
						inherit_global: false,
					},
					[fixtureCatalog.slotString.s153()]: {
						node_id: fixtureCatalog.slotString.s153(),
						inherit_global: false,
					},
				},
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};
