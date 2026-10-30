import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function statusEventsFixture() {
	const hello = {
		node_id: fixtureCatalog.identifier.nodeSecondary(),
		connected_at: fixtureCatalog.timestamp.recent(),
	};
	const snapshot = {
		emitted_at: fixtureCatalog.timestamp.recent(),
		health: { status: "ok" },
		cluster_info: {
			cluster_id: fixtureCatalog.cluster.fixture84(),
			node_id: fixtureCatalog.identifier.nodeSecondary(),
			role: "leader",
			leader_api_base_url: fixtureCatalog.url.secondaryApi(),
			term: 73,
			xp_version: "v3.22.5",
		},
		nodes_runtime: { partial: false, unreachable_nodes: [], items: [] },
		alerts: { partial: false, unreachable_nodes: [], items: [] },
		upgrade: {
			support: { supported: false },
			status: { state: "idle", updated_at: fixtureCatalog.timestamp.recent() },
		},
	};
	return [
		`event: hello\ndata: ${JSON.stringify(hello)}\n\n`,
		`event: snapshot\ndata: ${JSON.stringify(snapshot)}\n\n`,
	].join("");
}
