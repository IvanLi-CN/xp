import { describe, expect, it } from "vitest";
import { buildEndpointCreateMeta } from "../../.storybook/mocks/buildEndpointCreateMeta";
import { normalizeFixtureEndpointPatch } from "../../.storybook/mocks/fixtureMutationPolicy";
import { type AdminEndpoint, RealityConfigSchema } from "../api/adminEndpoints";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	DEFAULT_XMUX_DRAFT,
	xmuxDraftFromValue,
	xmuxFromDraft,
} from "./mihomoXmux";

describe("XMUX draft", () => {
	it("keeps existing defaults and produces an exclusive concurrency policy", () => {
		expect(xmuxFromDraft(DEFAULT_XMUX_DRAFT)).toEqual({
			max_connections: "1",
			max_concurrency: "0",
			c_max_reuse_times: "0",
			h_max_reusable_secs: "0",
			h_keep_alive_period: -1,
		});
		const draft = {
			...DEFAULT_XMUX_DRAFT,
			strategy: "concurrency" as const,
			reuseTimes: "20-40",
			keepAlive: "45",
		};
		const config = xmuxFromDraft(draft);
		expect(config.max_connections).toBe("0");
		expect(config.max_concurrency).toBe("16-32");
		expect(xmuxDraftFromValue(config)).toEqual(draft);
		expect(
			xmuxFromDraft({ ...draft, reuseTimes: "0-2", reusableSecs: "0-60" }),
		).toMatchObject({ c_max_reuse_times: "0-2", h_max_reusable_secs: "0-60" });
	});
	it("rejects malformed ranges and invalid keepalive", () => {
		for (const value of ["", "-1", "8-2", "0-1", "01", "2147483648"]) {
			expect(() =>
				xmuxFromDraft({ ...DEFAULT_XMUX_DRAFT, maxConnections: value }),
			).toThrow();
		}
		expect(() =>
			xmuxFromDraft({ ...DEFAULT_XMUX_DRAFT, keepAlive: "-2" }),
		).toThrow();
	});
	it("preserves custom policy in Storybook create and update responses", () => {
		const policy = xmuxFromDraft({
			...DEFAULT_XMUX_DRAFT,
			strategy: "concurrency",
		});
		const meta = buildEndpointCreateMeta(
			{
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture118(),
				port: fixtureCatalog.endpoint.port53844(),
				reality: RealityConfigSchema.parse(fixtureCatalog.endpoint.reality()),
				mihomo_xmux: policy,
			},
			[],
		);
		expect(meta.mihomo_xmux).toEqual(policy);
		const managed = buildEndpointCreateMeta(
			{
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture118(),
				port: fixtureCatalog.endpoint.port53844(),
				mihomo_xmux: policy,
			},
			[
				{
					node_id: fixtureCatalog.nodeId.fixture118(),
					node_name: fixtureCatalog.nodeName.fixture33(),
					access_host: fixtureCatalog.host.fixture119(),
					api_base_url: fixtureCatalog.service.fixture34(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetNode(),
				},
			],
		);
		expect(managed.mihomo_xmux).toEqual(policy);
		const updated = normalizeFixtureEndpointPatch<AdminEndpoint>(
			{
				endpoint_id: fixtureCatalog.endpointId.fixture120(),
				node_id: fixtureCatalog.nodeId.fixture118(),
				tag: fixtureCatalog.endpointTag.fixture121(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port53844(),
				meta: {},
			},
			{ mihomo_xmux: policy },
		);
		expect(updated?.meta.mihomo_xmux).toEqual(policy);
		if (!updated) throw new Error("fixture patch rejected a valid policy");
		const cleared = normalizeFixtureEndpointPatch(updated, {
			mihomo_xmux: null,
		});
		expect(cleared?.meta).not.toHaveProperty("mihomo_xmux");
	});
});
