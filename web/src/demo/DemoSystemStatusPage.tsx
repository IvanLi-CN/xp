import type { AdminMeshStatus } from "@/api/adminMesh";
import { Button } from "@/components/Button";
import { Icon } from "@/components/Icon";
import { SystemStatusSurface } from "@/views/SystemStatusPage";

const demoNavigation = [
	["Dashboard", "tabler:layout-dashboard"],
	["System status", "tabler:activity-heartbeat"],
	["Nodes", "tabler:server"],
	["Endpoints", "tabler:plug"],
	["Users", "tabler:users"],
] as const;

const demoSettingsNavigation = [
	["Quota policy", "tabler:gauge"],
	["Service config", "tabler:settings"],
	["Tools", "tabler:tool"],
] as const;

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
		mesh_h2_requests: item === "mesh" || item === "slow" ? 1 : 0,
		mesh_connection_starts: 0,
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
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
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
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 3,
				current_connection_requests: 58,
				requests_5m: 58,
				connection_starts_5m: 1,
				requests_1h: 684,
				connection_starts_1h: 2,
				last_connection_started_at: "2026-08-03T09:03:00Z",
			},
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
			mesh_capability: "enabled",
			mesh_reason: "fallback_active",
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
			mesh_transport: {
				protocol: "h2",
				health: "churning",
				connection_generation: 18,
				current_connection_requests: 2,
				requests_5m: 17,
				connection_starts_5m: 4,
				requests_1h: 219,
				connection_starts_1h: 18,
				last_connection_started_at: "2026-08-03T09:46:00Z",
			},
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
			mesh_capability: "disabled",
			mesh_reason: "missing_endpoint",
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

export function DemoSystemStatusPage({
	status = demoMeshStatus,
}: { status?: AdminMeshStatus }) {
	return (
		<SystemStatusSurface
			status={status}
			showMeshTransportReuse
			components={[
				{ component: "xp", status: "up" },
				{ component: "xray", status: "up" },
				{ component: "cloudflared", status: "up" },
				{ component: "ddns", status: "up" },
			]}
		/>
	);
}

export const demoReverseMeshStatus: AdminMeshStatus = {
	...demoMeshStatus,
	peers: [
		{
			...demoMeshStatus.peers[1],
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "good",
			breaker: "closed",
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 4,
				current_connection_requests: 46,
				requests_5m: 46,
				connection_starts_5m: 1,
				requests_1h: 552,
				connection_starts_1h: 2,
				last_connection_started_at: "2026-08-03T09:16:00Z",
			},
			buckets: buckets(Array<"mesh">(10).fill("mesh")),
		},
		{
			...demoMeshStatus.peers[2],
			mesh_url: "https://syd-1.edge.example.net:443",
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "good",
			breaker: "closed",
			last_sample_at: "2026-08-03T09:46:00Z",
			last_transition_at: "2026-08-03T08:14:00Z",
			availability_1h: 1,
			availability_24h: 0.997,
			mesh_availability_24h: 0.997,
			latency_p50_ms: 37,
			latency_p95_ms: 55,
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 2,
				current_connection_requests: 31,
				requests_5m: 31,
				connection_starts_5m: 1,
				requests_1h: 372,
				connection_starts_1h: 2,
				last_connection_started_at: "2026-08-03T09:14:00Z",
			},
			buckets: buckets(Array<"mesh">(10).fill("mesh")),
		},
		{
			...demoMeshStatus.peers[0],
			active_route: {
				kind: "reverse_relay",
				rendezvous: "node-sgp-1",
				rendezvous_role: "primary",
				primary_rendezvous: "node-sgp-1",
				standby_rendezvous: "node-syd-1",
				generation: 7,
				readiness: "active",
			},
		},
		{
			...demoMeshStatus.peers[0],
			node_id: "node-seoul-1",
			node_name: "seoul-1",
			api_base_url: "https://seoul-1.edge.example.net",
			mesh_url: null,
			mesh_capability: "disabled",
			mesh_reason: "missing_endpoint",
			last_sample_at: "2026-08-03T09:45:00Z",
			last_transition_at: "2026-08-03T08:12:00Z",
			availability_1h: 0.995,
			availability_24h: 0.996,
			mesh_availability_24h: 0.993,
			latency_p50_ms: 41,
			latency_p95_ms: 62,
			active_route: {
				kind: "reverse_relay",
				rendezvous: "node-sgp-1",
				rendezvous_role: "primary",
				primary_rendezvous: "node-sgp-1",
				standby_rendezvous: "node-syd-1",
				generation: 9,
				readiness: "active",
			},
		},
	],
	events: [
		{
			at: "2026-08-03T09:48:00Z",
			peer_id: "node-osaka-1",
			kind: "transition",
			message:
				"Reverse targets use singapore-1 as primary and sydney-1 as standby.",
		},
		...demoMeshStatus.events,
	],
};

export function UiDemoSystemStatusPage() {
	return (
		<div className="min-h-screen bg-background">
			<div className="p-1.5 sm:p-4 md:p-5">
				<div className="mx-auto flex max-w-7xl flex-col gap-2.5 sm:gap-4">
					<header className="xp-panel px-2 py-2 sm:px-4 sm:py-3">
						<div className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-4">
							<div className="flex min-w-0 items-center gap-2">
								<img src="/xp-mark.png" alt="" className="size-6 shrink-0" />
								<span className="text-lg font-semibold">xp</span>
								<span className="hidden text-sm text-muted-foreground sm:inline">
									cluster manager
								</span>
							</div>
							<div className="hidden justify-center lg:flex">
								<div
									className={
										"w-full max-w-md rounded-full border border-border bg-muted/40 px-4 " +
										"py-2 font-mono text-sm text-muted-foreground"
									}
								>
									Search / command palette
								</div>
							</div>
							<div className="flex items-center gap-2">
								<span
									className={
										"hidden rounded-full border border-border px-2 py-1 font-mono " +
										"text-xs lg:inline"
									}
								>
									xp 3.23.1
								</span>
								<Button variant="secondary" size="sm">
									<Icon name="tabler:activity-heartbeat" size={15} />
									<span className="hidden sm:inline">Status</span>
								</Button>
								<Button variant="secondary" size="sm">
									<Icon name="tabler:settings" size={15} />
									<span className="hidden sm:inline">Settings</span>
								</Button>
							</div>
						</div>
					</header>

					<div className="grid grid-cols-1 gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
						<aside className="xp-panel hidden p-4 lg:block">
							<DemoNavigation title="Nav" items={demoNavigation} />
							<DemoNavigation title="Settings" items={demoSettingsNavigation} />
						</aside>
						<main className="xp-panel min-w-0 p-[var(--xp-page-padding)]">
							<DemoSystemStatusPage status={demoReverseMeshStatus} />
						</main>
					</div>
				</div>
			</div>
		</div>
	);
}

function DemoNavigation({
	title,
	items,
}: {
	title: string;
	items: ReadonlyArray<readonly [string, string]>;
}) {
	return (
		<div className="mb-6 space-y-2 last:mb-0">
			<p className="px-2 text-xs uppercase tracking-[0.18em] text-muted-foreground">
				{title}
			</p>
			<ul className="space-y-1.5">
				{items.map(([label, icon]) => (
					<li key={label}>
						<div
							className={`flex items-center gap-3 rounded-xl border px-3 py-2 text-sm font-medium ${
								label === "System status"
									? "border-primary/25 bg-primary/10 text-foreground"
									: "border-transparent text-muted-foreground"
							}`}
						>
							<Icon name={icon} className="size-5 opacity-80" />
							<span>{label}</span>
						</div>
					</li>
				))}
			</ul>
		</div>
	);
}
