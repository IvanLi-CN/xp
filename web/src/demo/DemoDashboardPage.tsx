import { Link } from "@tanstack/react-router";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { useToast } from "../components/Toast";
import { buttonVariants } from "../components/ui/button";
import {
	formatGb,
	formatPercent,
	nodeStatusVariant,
	shortDate,
} from "./format";
import { useDemo } from "./store";

function MetricCard({
	label,
	value,
	meta,
	icon,
}: {
	label: string;
	value: string;
	meta: string;
	icon: string;
}) {
	return (
		<div className="xp-panel-muted p-4">
			<div className="flex items-start justify-between gap-3">
				<div>
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						{label}
					</p>
					<p className="mt-2 text-2xl font-semibold">{value}</p>
					<p className="mt-1 text-sm text-muted-foreground">{meta}</p>
				</div>
				<Icon name={icon} className="size-5 text-primary" ariaLabel={label} />
			</div>
		</div>
	);
}

export function DemoDashboardPage() {
	const { state } = useDemo();
	const { pushToast } = useToast();
	const [refreshing, setRefreshing] = useState(false);
	const canWrite = state.session?.role !== "viewer";
	const healthyNodes = state.nodes.filter(
		(node) => node.status === "healthy",
	).length;
	const servingEndpoints = state.endpoints.filter(
		(endpoint) => endpoint.status === "serving",
	).length;
	const quotaLimited = state.users.filter(
		(user) => user.status === "quota_limited",
	).length;
	const finiteQuotaUsers = state.users.filter(
		(user) => user.quotaLimitGb !== null,
	);
	const unlimitedUsers = state.users.length - finiteQuotaUsers.length;
	const usedGb = finiteQuotaUsers.reduce(
		(sum, user) => sum + user.quotaUsedGb,
		0,
	);
	const limitGb = finiteQuotaUsers.reduce(
		(sum, user) => sum + (user.quotaLimitGb ?? 0),
		0,
	);
	const quotaMeta =
		limitGb === 0
			? unlimitedUsers > 0
				? `${unlimitedUsers} unlimited ${unlimitedUsers === 1 ? "user" : "users"}`
				: "no finite user quota"
			: `${formatGb(usedGb)} finite used / ${formatGb(limitGb)}${
					unlimitedUsers > 0
						? `, ${unlimitedUsers} unlimited ${
								unlimitedUsers === 1 ? "user" : "users"
							}`
						: ""
				}`;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Demo dashboard"
				description="Walk through the cluster manager with deterministic mock data."
				actions={
					<>
						<Button
							variant="secondary"
							loading={refreshing}
							onClick={() => {
								setRefreshing(true);
								window.setTimeout(() => {
									setRefreshing(false);
									pushToast({
										variant: "success",
										message: "Demo cluster snapshot refreshed.",
									});
								}, 700);
							}}
						>
							Refresh snapshot
						</Button>
						{canWrite ? (
							<Link className={buttonVariants()} to="/demo/endpoints/new">
								New endpoint
							</Link>
						) : (
							<Button disabled>New endpoint</Button>
						)}
					</>
				}
			/>

			<div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
				<MetricCard
					label="Nodes"
					value={`${healthyNodes}/${state.nodes.length}`}
					meta="healthy nodes"
					icon="tabler:server"
				/>
				<MetricCard
					label="Endpoints"
					value={`${servingEndpoints}/${state.endpoints.length}`}
					meta="serving"
					icon="tabler:plug"
				/>
				<MetricCard
					label="Users"
					value={state.users.length.toLocaleString()}
					meta={`${quotaLimited} quota-limited`}
					icon="tabler:users"
				/>
				<MetricCard
					label="Quota"
					value={limitGb === 0 ? "unlimited" : formatPercent(usedGb, limitGb)}
					meta={quotaMeta}
					icon="tabler:gauge"
				/>
			</div>

			<div className="grid min-w-0 gap-6 xl:grid-cols-[minmax(0,1.1fr)_minmax(20rem,0.9fr)]">
				<section className="xp-card">
					<div className="xp-card-body">
						<div className="flex items-center justify-between gap-3">
							<h2 className="xp-card-title">Cluster facts</h2>
							<Link className="xp-link text-sm" to="/demo/nodes">
								View nodes
							</Link>
						</div>
						<div className="xp-table-wrap">
							<table className="xp-table xp-table-zebra">
								<thead>
									<tr>
										<th>Node</th>
										<th>Role</th>
										<th>Status</th>
										<th>Latency</th>
										<th>Quota</th>
									</tr>
								</thead>
								<tbody>
									{state.nodes.map((node) => (
										<tr key={node.id}>
											<td>
												<Link
													className="font-medium hover:underline"
													to="/demo/nodes/$nodeId"
													params={{ nodeId: node.id }}
												>
													{node.name}
												</Link>
												<p className="font-mono text-xs text-muted-foreground">
													{node.accessHost}
												</p>
											</td>
											<td className="font-mono text-xs">{node.role}</td>
											<td>
												<Badge
													variant={nodeStatusVariant(node.status)}
													size="sm"
												>
													{node.status}
												</Badge>
											</td>
											<td className="font-mono text-xs">
												{node.latencyMs === null ? "-" : `${node.latencyMs} ms`}
											</td>
											<td>
												<div className="min-w-32">
													<p className="font-mono text-xs">
														{formatGb(node.quotaUsedGb)} /{" "}
														{formatGb(node.quotaLimitGb)}
													</p>
													<div className="mt-1 h-2 rounded-full bg-muted">
														<div
															className="h-full origin-left rounded-full bg-primary transition-transform"
															style={{
																transform: `scaleX(${
																	node.quotaLimitGb
																		? Math.min(
																				1,
																				node.quotaUsedGb / node.quotaLimitGb,
																			)
																		: 0.22
																})`,
															}}
														/>
													</div>
												</div>
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					</div>
				</section>

				<section className="xp-card">
					<div className="xp-card-body">
						<div className="flex items-center justify-between gap-3">
							<h2 className="xp-card-title">Activity</h2>
							<Link className="xp-link text-sm" to="/demo/scenarios">
								Demo scripts
							</Link>
						</div>
						<div className="space-y-3">
							{state.activity.map((item) => (
								<div
									key={item.id}
									className="rounded-xl border border-border/70 bg-muted/30 px-3 py-2"
								>
									<div className="flex items-center justify-between gap-3">
										<Badge
											variant={
												item.kind === "error"
													? "destructive"
													: item.kind === "warning"
														? "warning"
														: item.kind === "success"
															? "success"
															: "info"
											}
											size="sm"
										>
											{item.kind}
										</Badge>
										<span className="font-mono text-xs text-muted-foreground">
											{shortDate(item.at)}
										</span>
									</div>
									<p className="mt-2 text-sm">{item.message}</p>
								</div>
							))}
						</div>
					</div>
				</section>
			</div>
		</div>
	);
}
