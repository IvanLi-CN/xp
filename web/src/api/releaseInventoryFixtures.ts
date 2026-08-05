export type ReleaseFixtureResponse = {
	contentType: "application/json" | "text/event-stream" | "text/plain";
	body: unknown;
	wireBody?: string;
};

const STATUS_EVENT_HELLO = {
	connected_at: "2026-08-05T00:00:00Z",
	node_id: "01fixture000000000000000000",
};

const STATUS_EVENT_SNAPSHOT = {
	alerts: { items: [], partial: false, unreachable_nodes: [] },
	cluster_info: {
		cluster_id: "fixture-cluster",
		leader_api_base_url: "https://fixture.example",
		node_id: "01fixture000000000000000000",
		role: "leader",
		term: 7,
		xp_version: "3.22.5",
	},
	emitted_at: "2026-08-05T00:00:00Z",
	health: { status: "ok" },
	nodes_runtime: { items: [], partial: false, unreachable_nodes: [] },
	upgrade: {
		status: { state: "idle", updated_at: "2026-08-05T00:00:00Z" },
		support: { supported: false },
	},
};

export function statusEventFixture(
	meshRevision?: number,
): ReleaseFixtureResponse {
	const snapshot =
		meshRevision === undefined
			? STATUS_EVENT_SNAPSHOT
			: { ...STATUS_EVENT_SNAPSHOT, mesh_revision: meshRevision };
	const body = { hello: STATUS_EVENT_HELLO, snapshot };
	return {
		contentType: "text/event-stream",
		body,
		wireBody: [
			"event: hello",
			`data: ${JSON.stringify(body.hello)}`,
			"",
			"event: snapshot",
			`data: ${JSON.stringify(body.snapshot)}`,
			"",
		].join("\n"),
	};
}
