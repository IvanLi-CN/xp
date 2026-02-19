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
			tableClassName="table-fixed w-full"
			headers={[
				{ key: "probe", label: "Probe (24h)", className: "w-40" },
				{
					key: "latency",
					align: "right",
					className: "w-24",
					label: (
						<div className="flex flex-col leading-tight">
							<span>Latency</span>
							<span className="text-xs opacity-60 font-normal whitespace-nowrap">
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
							<span className="text-xs opacity-60 font-normal whitespace-nowrap">
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
							<span className="text-xs opacity-60 font-normal whitespace-nowrap">
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
							className="inline-flex items-center"
							to="/endpoints/$endpointId/probe"
							params={{ endpointId: endpoint.endpoint_id }}
						>
							<EndpointProbeBar slots={endpoint.probe?.slots ?? []} />
						</Link>
					</td>
					<td className="font-mono text-xs text-right tabular-nums truncate">
						{endpoint.probe?.latest_latency_ms_p50 ?? "-"}
					</td>
					<td className="align-top">
						<div className="flex flex-col gap-1 min-w-0">
							<div className="flex items-center gap-2 min-w-0">
								<Link
									className="link link-primary font-mono text-xs block truncate min-w-0 whitespace-nowrap"
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
									className="px-2 shrink-0"
								/>
							</div>
							<div
								className="text-xs opacity-60 whitespace-nowrap truncate"
								title={endpoint.kind}
							>
								{formatKindShort(endpoint.kind)}
							</div>
						</div>
					</td>
					<td className="align-top">
						<div className="flex flex-col gap-1 min-w-0">
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
										className="text-xs font-medium block truncate min-w-0 whitespace-nowrap"
										title={nodeTitle}
									>
										{nodeLabel}
									</div>
								);
							})()}
							<div
								className="font-mono text-xs opacity-60 whitespace-nowrap truncate"
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
