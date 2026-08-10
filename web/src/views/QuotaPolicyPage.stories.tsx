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
						node_id: fixtureCatalog.nodeId.fixture145(),
						node_name: fixtureCatalog.nodeName.fixture146(),
						access_host: fixtureCatalog.host.fixture147(),
						api_base_url: fixtureCatalog.service.fixture148(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
					{
						node_id: fixtureCatalog.nodeId.fixture149(),
						node_name: fixtureCatalog.nodeName.fixture150(),
						access_host: fixtureCatalog.host.fixture151(),
						api_base_url: fixtureCatalog.service.fixture152(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
					{
						node_id: fixtureCatalog.nodeId.fixture153(),
						node_name: fixtureCatalog.nodeName.fixture154(),
						access_host: fixtureCatalog.host.fixture155(),
						api_base_url: fixtureCatalog.service.fixture156(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture157(),
						node_id: fixtureCatalog.nodeId.fixture145(),
						tag: fixtureCatalog.endpointTag.fixture158(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port443(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39159(),
								server_names: fixtureCatalog.hostList.edge9(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture160(),
						node_id: fixtureCatalog.nodeId.fixture145(),
						tag: fixtureCatalog.endpointTag.fixture161(),
						kind: fixtureCatalog.endpoint.ssKind(),
						port: fixtureCatalog.endpoint.port8443(),
						meta: {
							method: "2022-blake3-aes-128-gcm",
						},
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture162(),
						node_id: fixtureCatalog.nodeId.fixture149(),
						tag: fixtureCatalog.endpointTag.fixture163(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port444(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39164(),
								server_names: fixtureCatalog.hostList.edge10(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture165(),
						node_id: fixtureCatalog.nodeId.fixture153(),
						tag: fixtureCatalog.endpointTag.fixture166(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port445(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39167(),
								server_names: fixtureCatalog.hostList.edge11(),
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
						subscription_token: fixtureCatalog.token.fixture168(),
						credential_epoch: 0,
						priority_tier: "p1",
						quota_reset: fixtureCatalog.quota.reset(),
					},
					{
						user_id: userIdB,
						display_name: "Koha",
						subscription_token: fixtureCatalog.token.fixture169(),
						credential_epoch: 0,
						priority_tier: "p2",
						quota_reset: fixtureCatalog.quota.reset(),
					},
				],
				userNodeWeights: Object.fromEntries([
					[
						userIdA,
						[
							{ node_id: fixtureCatalog.nodeId.fixture145(), weight: 6500 },
							{ node_id: fixtureCatalog.nodeId.fixture149(), weight: 5000 },
							{ node_id: fixtureCatalog.nodeId.fixture153(), weight: 2000 },
						],
					],
					[
						userIdB,
						[
							{ node_id: fixtureCatalog.nodeId.fixture145(), weight: 3500 },
							{ node_id: fixtureCatalog.nodeId.fixture149(), weight: 5000 },
							{ node_id: fixtureCatalog.nodeId.fixture153(), weight: 8000 },
						],
					],
				]),
				userGlobalWeights: Object.fromEntries([
					[userIdA, 6500],
					[userIdB, 3500],
				]),
				nodeWeightPolicies: {
					[fixtureCatalog.nodeId.fixture145()]: {
						node_id: fixtureCatalog.nodeId.fixture145(),
						inherit_global: false,
					},
					[fixtureCatalog.nodeId.fixture149()]: {
						node_id: fixtureCatalog.nodeId.fixture149(),
						inherit_global: false,
					},
					[fixtureCatalog.nodeId.fixture153()]: {
						node_id: fixtureCatalog.nodeId.fixture153(),
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
