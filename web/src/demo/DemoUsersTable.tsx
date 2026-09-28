import { Link } from "@tanstack/react-router";

import { Badge } from "@/components/ui/badge";

import { CopyButton } from "../components/CopyButton";
import { TableScrollFrame } from "../components/TableScrollFrame";
import {
	formatGb,
	formatPercent,
	subscriptionUrl,
	userStatusVariant,
} from "./format";
import type { DemoUser } from "./types";

export function DemoUsersTable({ users }: { users: DemoUser[] }) {
	return (
		<>
			<div className="hidden sm:block">
				<TableScrollFrame ariaLabel="Demo users">
					<table className="xp-table xp-table-zebra">
						<thead>
							<tr>
								<th>User</th>
								<th>Status</th>
								<th>Tier</th>
								<th>Quota</th>
								<th>Endpoints</th>
								<th>Subscription</th>
							</tr>
						</thead>
						<tbody>
							{users.map((user) => (
								<tr key={user.id}>
									<td>
										<Link
											className="font-medium hover:underline"
											to="/demo/users/$userId"
											params={{ userId: user.id }}
										>
											{user.displayName}
										</Link>
										<p className="max-w-72 truncate text-xs text-muted-foreground">
											{user.email}
										</p>
									</td>
									<td>
										<Badge variant={userStatusVariant(user.status)} size="sm">
											{user.status}
										</Badge>
									</td>
									<td className="font-mono text-xs">{user.tier}</td>
									<td>
										<p className="font-mono text-xs">
											{formatGb(user.quotaUsedGb)} /{" "}
											{formatGb(user.quotaLimitGb)}
										</p>
										<p className="text-xs text-muted-foreground">
											{formatPercent(user.quotaUsedGb, user.quotaLimitGb)}
										</p>
									</td>
									<td className="font-mono text-xs">
										{user.endpointIds.length}
									</td>
									<td>
										<CopyButton
											text={subscriptionUrl(user.subscriptionToken)}
											label="Copy"
											ariaLabel={`Copy subscription URL for ${user.displayName}`}
											size="sm"
										/>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</TableScrollFrame>
			</div>

			<ul className="space-y-3 sm:hidden" aria-label="Users">
				{users.map((user) => (
					<li
						key={user.id}
						className="rounded-xl border border-border/70 bg-card p-3 shadow-sm"
						data-testid="demo-user-mobile-card"
					>
						<div className="flex min-w-0 items-start justify-between gap-3">
							<div className="min-w-0">
								<Link
									className="block truncate font-semibold text-primary hover:underline"
									to="/demo/users/$userId"
									params={{ userId: user.id }}
									title={user.displayName}
								>
									{user.displayName}
								</Link>
								<p
									className="mt-1 break-all text-xs text-muted-foreground"
									title={user.email}
								>
									{user.email}
								</p>
							</div>
							<CopyButton
								text={subscriptionUrl(user.subscriptionToken)}
								iconOnly
								variant="ghost"
								size="sm"
								ariaLabel={`Copy subscription URL for ${user.displayName}`}
								className="shrink-0 px-2"
							/>
						</div>

						<div className="mt-3 flex items-center justify-between gap-3 border-t border-border/60 pt-3">
							<Badge variant={userStatusVariant(user.status)} size="sm">
								{user.status}
							</Badge>
							<span className="font-mono text-xs text-muted-foreground">
								{user.tier}
							</span>
						</div>

						<div className="mt-3 grid grid-cols-2 gap-3 text-xs">
							<div>
								<p className="text-muted-foreground">Quota</p>
								<p className="mt-1 font-mono">
									{formatGb(user.quotaUsedGb)} / {formatGb(user.quotaLimitGb)}
								</p>
								<p className="text-muted-foreground">
									{formatPercent(user.quotaUsedGb, user.quotaLimitGb)}
								</p>
							</div>
							<div className="text-right">
								<p className="text-muted-foreground">Endpoints</p>
								<p className="mt-1 font-mono">{user.endpointIds.length}</p>
							</div>
						</div>
					</li>
				))}
			</ul>
		</>
	);
}
