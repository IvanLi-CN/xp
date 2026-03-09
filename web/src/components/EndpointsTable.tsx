import { Link } from "@tanstack/react-router";

import type { AdminEndpoint, AdminEndpointKind } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { CopyButton } from "./CopyButton";
import { EndpointProbeBar } from "./EndpointProbeBar";
import { ResourceTable } from "./ResourceTable";

function formatKindShort(kind: AdminEndpointKind): string {
	switch (kind) {
		case "vless_reality_vision_tcp":
			return "VLESS";
		case "ss2022_2022_blake3_aes_128_gcm":
			return "SS2022";
		default:
			return kind;
	}
}

export function EndpointsTable(props: {
	endpoints: AdminEndpoint[];
	nodeById?: Map<string, AdminNode>;
}) {
	const { endpoints, nodeById } = props;

	return (
		<ResourceTable
			tableClassName="w-full table-fixed"
			headers={[
				{ key: "probe", label: "Probe (24h)", className: "w-40" },
				{
					key: "latency",
					align: "right",
					className: "w-24",
					label: (
						<div className="flex flex-col leading-tight">
							<span>Latency</span>
							<span className="whitespace-nowrap text-xs font-normal text-muted-foreground">
								p50 ms
							</span>
						</div>
					),
				},
				{
					key: "endpoint",
					label: (
						<div className="flex flex-col leading-tight">
							<span>Endpoint</span>
							<span className="whitespace-nowrap text-xs font-normal text-muted-foreground">
								tag/kind
							</span>
						</div>
					),
				},
				{
					key: "node",
					label: (
						<div className="flex flex-col leading-tight">
							<span>Node</span>
							<span className="whitespace-nowrap text-xs font-normal text-muted-foreground">
								name/port
							</span>
						</div>
					),
				},
			]}
		>
			{endpoints.map((endpoint) => (
				<tr key={endpoint.endpoint_id}>
					<td>
						<Link
							className="inline-flex items-center rounded-md outline-none transition-opacity hover:opacity-80 focus-visible:ring-2 focus-visible:ring-ring/50"
							to="/endpoints/$endpointId/probe"
							params={{ endpointId: endpoint.endpoint_id }}
						>
							<EndpointProbeBar slots={endpoint.probe?.slots ?? []} />
						</Link>
					</td>
					<td className="truncate text-right font-mono text-xs tabular-nums">
						{endpoint.probe?.latest_latency_ms_p50 ?? "-"}
					</td>
					<td className="align-top">
						<div className="flex min-w-0 flex-col gap-1">
							<div className="flex min-w-0 items-center gap-2">
								<Link
									className="xp-link block min-w-0 truncate whitespace-nowrap font-mono text-xs"
									to="/endpoints/$endpointId"
									params={{ endpointId: endpoint.endpoint_id }}
									title={endpoint.tag}
								>
									{endpoint.tag}
								</Link>
								<CopyButton
									text={endpoint.endpoint_id}
									iconOnly
									variant="ghost"
									size="sm"
									ariaLabel="Copy endpoint ID"
									className="shrink-0 px-2"
								/>
							</div>
							<div
								className="truncate whitespace-nowrap text-xs text-muted-foreground"
								title={endpoint.kind}
							>
								{formatKindShort(endpoint.kind)}
							</div>
						</div>
					</td>
					<td className="align-top">
						<div className="flex min-w-0 flex-col gap-1">
							{(() => {
								const node = nodeById?.get(endpoint.node_id);
								const nodeName = node?.node_name?.trim() ?? "";
								const nodeTitle =
									nodeName.length > 0
										? `${nodeName} (${endpoint.node_id})`
										: endpoint.node_id;
								const nodeLabel =
									nodeName.length > 0 ? nodeName : endpoint.node_id;

								return (
									<div
										className="block min-w-0 truncate whitespace-nowrap text-xs font-medium"
										title={nodeTitle}
									>
										{nodeLabel}
									</div>
								);
							})()}
							<div
								className="truncate whitespace-nowrap font-mono text-xs text-muted-foreground"
								title={String(endpoint.port)}
							>
								{endpoint.port}
							</div>
						</div>
					</td>
				</tr>
			))}
		</ResourceTable>
	);
}
