import type { AdminMeshStatus } from "@/api/adminMesh";
import { SystemStatusSurface } from "@/views/SystemStatusPage";

function buckets(pattern: Array<"mesh" | "fallback" | "slow" | "down">) {
	return pattern.map((item, index) => ({
		minute:
			`2026-08-03T${String(Math.floor(index / 60)).padStart(2, "0")}:` +
			`${String(index % 60).padStart(2, "0")}:00Z`,
		mesh_success: item === "mesh" || item === "slow" ? 1 : 0,
		mesh_failure: item === "fallback" || item === "down" ? 1 : 0,
		public_success: item === "fallback" ? 1 : 0,
		public_failure: item === "down" ? 1 : 0,
		fallback_success: item === "fallback" ? 1 : 0,
		end_to_end_success: item === "down" ? 0 : 1,
		end_to_end_failure: item === "down" ? 1 : 0,
		latency_samples_ms: [
			item === "slow" ? 680 : item === "fallback" ? 190 : 36,
		],
	}));
}

export const demoMeshStatus: AdminMeshStatus = {
	generated_at: "2026-08-03T09:48:00Z",
	revision: 74,
	local: {
		node_id: "node-tokyo-1",
		node_name: "tokyo-1",
		cluster_id: "01JXPDEMOCLUSTER000000000000",
		role: "leader",
		leader_api_base_url: "https://tokyo-1.edge.example.net",
		term: 42,
		mesh_proxy_status: "ready",
		mesh_proxy_reason: null,
		canary: {
			enabled: true,
			bind: "127.0.0.1:39043",
			acme_directory_url: null,
			cert_not_after: "2026-11-01T00:00:00Z",
			last_renewed_at: "2026-08-01T00:00:00Z",
			last_error: null,
		},
	},
	peers: [
		{
			node_id: "node-osaka-1",
			node_name: "osaka-1",
			api_base_url: "https://osaka-1.edge.example.net",
			mesh_url: "https://osaka-1.edge.example.net:443",
			current_path: "mesh",
			quality: "good",
			stale: false,
			breaker: "closed",
			last_sample_at: "2026-08-03T09:47:00Z",
			last_transition_at: "2026-08-03T08:20:00Z",
			availability_1h: 1,
			availability_24h: 0.999,
			mesh_availability_24h: 0.998,
			latency_p50_ms: 24,
			latency_p95_ms: 39,
			buckets: buckets([
				"mesh",
				"mesh",
				"mesh",
				"fallback",
				"mesh",
				"mesh",
				"mesh",
				"mesh",
				"mesh",
				"mesh",
			]),
		},
		{
			node_id: "node-sgp-1",
			node_name: "singapore-1",
			api_base_url: "https://sgp-1.edge.example.net",
			mesh_url: "https://sgp-1.edge.example.net:443",
			current_path: "public",
			quality: "slow",
			stale: false,
			breaker: "open",
			last_sample_at: "2026-08-03T09:46:00Z",
			last_transition_at: "2026-08-03T09:32:00Z",
			availability_1h: 0.983,
			availability_24h: 0.991,
			mesh_availability_24h: 0.862,
			latency_p50_ms: 184,
			latency_p95_ms: 680,
			buckets: buckets([
				"mesh",
				"slow",
				"fallback",
				"fallback",
				"down",
				"fallback",
				"slow",
				"fallback",
				"fallback",
				"fallback",
			]),
		},
		{
			node_id: "node-syd-1",
			node_name: "sydney-1",
			api_base_url: "https://syd-1.edge.example.net",
			mesh_url: null,
			current_path: null,
			quality: "unknown",
			stale: false,
			breaker: "disabled",
			last_sample_at: null,
			last_transition_at: null,
			availability_1h: null,
			availability_24h: null,
			mesh_availability_24h: null,
			latency_p50_ms: null,
			latency_p95_ms: null,
			buckets: [],
		},
	],
	events: [
		{
			at: "2026-08-03T09:32:00Z",
			peer_id: "node-sgp-1",
			kind: "breaker",
			message: "Mesh breaker opened after retryable transport failure.",
		},
		{
			at: "2026-08-03T09:34:00Z",
			peer_id: "node-sgp-1",
			kind: "recovered",
			message:
				"Public fallback restored end-to-end control-plane reachability.",
		},
		{
			at: "2026-08-03T08:20:00Z",
			peer_id: "node-osaka-1",
			kind: "transition",
			message: "Reality Mesh resumed after a signed health acknowledgement.",
		},
	],
};

export function DemoSystemStatusPage() {
	return (
		<SystemStatusSurface
			status={demoMeshStatus}
			components={[
				{ component: "xp", status: "up" },
				{ component: "xray", status: "up" },
				{ component: "cloudflared", status: "up" },
				{ component: "ddns", status: "up" },
			]}
		/>
	);
}

export function UiDemoSystemStatusPage() {
	return (
		<div className="min-h-screen bg-background p-4 md:p-5">
			<main className="mx-auto max-w-7xl">
				<DemoSystemStatusPage />
			</main>
		</div>
	);
}
