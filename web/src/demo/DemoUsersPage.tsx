import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { CopyButton } from "../components/CopyButton";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { buttonVariants } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";
import {
	formatGb,
	formatPercent,
	shortDate,
	subscriptionUrl,
	userStatusVariant,
} from "./format";
import { useDemo } from "./store";
import type { DemoUser } from "./types";

export function DemoUsersPage() {
	const { state, undoDeleteUser } = useDemo();
	const { pushToast } = useToast();
	const [query, setQuery] = useState("");
	const [status, setStatus] = useState("all");
	const [sort, setSort] = useState("name");
	const [page, setPage] = useState(1);
	const pageSize = 6;
	const canWrite = state.session?.role !== "viewer";

	const filtered = useMemo(() => {
		const q = query.trim().toLowerCase();
		const items = state.users.filter((user) => {
			const matchesQuery =
				q.length === 0 ||
				user.displayName.toLowerCase().includes(q) ||
				user.email.toLowerCase().includes(q) ||
				user.subscriptionToken.toLowerCase().includes(q) ||
				user.locale.toLowerCase().includes(q);
			const matchesStatus = status === "all" || user.status === status;
			return matchesQuery && matchesStatus;
		});
		items.sort((a, b) => {
			if (sort === "quota") {
				const aRatio = a.quotaLimitGb ? a.quotaUsedGb / a.quotaLimitGb : 0;
				const bRatio = b.quotaLimitGb ? b.quotaUsedGb / b.quotaLimitGb : 0;
				return bRatio - aRatio;
			}
			if (sort === "tier") return a.tier.localeCompare(b.tier);
			return a.displayName.localeCompare(b.displayName);
		});
		return items;
	}, [query, sort, state.users, status]);

	const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
	const safePage = Math.min(page, pages);
	useEffect(() => {
		if (page > pages) setPage(pages);
	}, [page, pages]);
	const visible = filtered.slice(
		(safePage - 1) * pageSize,
		safePage * pageSize,
	);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Users"
				description="Search, sort, create, and review subscription access."
				actions={
					<>
						{canWrite ? (
							<Button asChild>
								<Link to="/demo/users/new">New user</Link>
							</Button>
						) : (
							<Button disabled>New user</Button>
						)}
						{state.lastDeletedUser ? (
							<Button
								variant="secondary"
								disabled={!canWrite}
								onClick={() => {
									if (!canWrite) return;
									undoDeleteUser();
									pushToast({ variant: "success", message: "User restored." });
								}}
							>
								Undo delete
							</Button>
						) : null}
					</>
				}
			/>

			<div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_10rem_10rem]">
				<Input
					value={query}
					onChange={(event) => {
						setQuery(event.target.value);
						setPage(1);
					}}
					placeholder="Search name, email, locale, or token"
					aria-label="Search users"
				/>
				<select
					className="xp-select"
					value={status}
					aria-label="Filter user status"
					onChange={(event) => {
						setStatus(event.target.value);
						setPage(1);
					}}
				>
					<option value="all">All statuses</option>
					<option value="active">Active</option>
					<option value="quota_limited">Quota limited</option>
					<option value="disabled">Disabled</option>
				</select>
				<select
					className="xp-select"
					value={sort}
					aria-label="Sort users"
					onChange={(event) => setSort(event.target.value)}
				>
					<option value="name">Name</option>
					<option value="quota">Quota pressure</option>
					<option value="tier">Priority tier</option>
				</select>
			</div>

			{state.users.length === 0 ? (
				<PageState
					variant="empty"
					title="No users yet"
					description="Create a user, then assign endpoints to build a subscription."
					action={
						canWrite ? (
							<Button asChild>
								<Link to="/demo/users/new">Create user</Link>
							</Button>
						) : (
							<Button disabled>Create user</Button>
						)
					}
				/>
			) : visible.length === 0 ? (
				<PageState
					variant="empty"
					title="No matching users"
					description="Try a different search or filter."
					action={
						<Button
							variant="secondary"
							onClick={() => {
								setQuery("");
								setStatus("all");
							}}
						>
							Clear filters
						</Button>
					}
				/>
			) : (
				<>
					<div className="xp-table-wrap">
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
								{visible.map((user) => (
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
					</div>
					<div className="flex items-center justify-between gap-3">
						<p className="text-sm text-muted-foreground">
							Page {safePage} of {pages}, {filtered.length} user(s)
						</p>
						<div className="flex gap-2">
							<Button
								variant="secondary"
								size="sm"
								disabled={safePage <= 1}
								onClick={() => setPage(Math.max(1, safePage - 1))}
							>
								Previous
							</Button>
							<Button
								variant="secondary"
								size="sm"
								disabled={safePage >= pages}
								onClick={() => setPage(Math.min(pages, safePage + 1))}
							>
								Next
							</Button>
						</div>
					</div>
				</>
			)}
		</div>
	);
}

export function DemoUserFormPage() {
	const navigate = useNavigate();
	const { state, createUser } = useDemo();
	const { pushToast } = useToast();
	const [displayName, setDisplayName] = useState("Nora Patel");
	const [email, setEmail] = useState("nora.patel@example.com");
	const [locale, setLocale] = useState("en-IN");
	const [tier, setTier] = useState<DemoUser["tier"]>("p2");
	const [quotaLimitGb, setQuotaLimitGb] = useState("160");
	const [endpointIds, setEndpointIds] = useState<string[]>(
		state.endpoints[0] ? [state.endpoints[0].id] : [],
	);
	const [submitted, setSubmitted] = useState(false);
	const [saving, setSaving] = useState(false);
	const canWrite = state.session?.role !== "viewer";
	const numericQuota = quotaLimitGb.trim() === "" ? null : Number(quotaLimitGb);
	const error =
		displayName.trim().length < 2
			? "Display name must be at least 2 characters."
			: !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)
				? "Email must look like a valid address."
				: numericQuota !== null &&
						(!Number.isFinite(numericQuota) || numericQuota < 0)
					? "Quota must be a non-negative number or empty for unlimited."
					: null;
	const dirty =
		displayName !== "Nora Patel" ||
		email !== "nora.patel@example.com" ||
		locale !== "en-IN" ||
		tier !== "p2" ||
		quotaLimitGb !== "160" ||
		endpointIds.length !== (state.endpoints[0] ? 1 : 0);

	return (
		<div className="space-y-6">
			<PageHeader
				title="New user"
				description="Create a user, assign endpoint access, and view the resulting subscription."
				meta={dirty ? <Badge variant="warning">dirty form</Badge> : null}
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/demo/users">Back</Link>
					</Button>
				}
			/>

			<form
				className="xp-card"
				onSubmit={(event) => {
					event.preventDefault();
					setSubmitted(true);
					if (error || !canWrite) return;
					setSaving(true);
					window.setTimeout(() => {
						const user = createUser({
							displayName,
							email,
							locale,
							tier,
							quotaLimitGb: numericQuota,
							endpointIds,
						});
						setSaving(false);
						pushToast({ variant: "success", message: "User created." });
						navigate({
							to: "/demo/users/$userId",
							params: { userId: user.id },
						});
					}, 550);
				}}
			>
				<div className="xp-card-body">
					<div className="grid gap-4 md:grid-cols-2">
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-user-name">
								Display name
							</label>
							<Input
								id="demo-user-name"
								value={displayName}
								onChange={(event) => setDisplayName(event.target.value)}
							/>
						</div>
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-user-email">
								Email
							</label>
							<Input
								id="demo-user-email"
								value={email}
								onChange={(event) => setEmail(event.target.value)}
							/>
						</div>
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-user-locale">
								Locale
							</label>
							<Input
								id="demo-user-locale"
								value={locale}
								onChange={(event) => setLocale(event.target.value)}
								className="font-mono"
							/>
						</div>
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-user-tier">
								Priority tier
							</label>
							<select
								id="demo-user-tier"
								className="xp-select"
								value={tier}
								onChange={(event) =>
									setTier(event.target.value as DemoUser["tier"])
								}
							>
								<option value="p1">p1</option>
								<option value="p2">p2</option>
								<option value="p3">p3</option>
							</select>
						</div>
					</div>

					<div className="xp-field-stack">
						<label className="text-sm font-medium" htmlFor="demo-user-quota">
							Quota limit GiB
						</label>
						<Input
							id="demo-user-quota"
							value={quotaLimitGb}
							inputMode="numeric"
							onChange={(event) => setQuotaLimitGb(event.target.value)}
							placeholder="empty means unlimited"
						/>
					</div>

					<div className="space-y-3 rounded-2xl border border-border/70 bg-muted/35 p-4">
						<div>
							<h2 className="text-sm font-semibold">Endpoint access</h2>
							<p className="text-xs text-muted-foreground">
								Selections are persisted to the mock subscription model.
							</p>
						</div>
						{state.endpoints.length === 0 ? (
							<PageState
								variant="empty"
								title="No endpoints available"
								description="Create an endpoint before assigning access."
								action={
									canWrite ? (
										<Button asChild>
											<Link to="/demo/endpoints/new">New endpoint</Link>
										</Button>
									) : (
										<Button disabled>New endpoint</Button>
									)
								}
							/>
						) : (
							<div className="grid gap-2 md:grid-cols-2">
								{state.endpoints.map((endpoint) => (
									<div
										key={endpoint.id}
										className="flex items-start gap-3 rounded-xl border border-border/70 bg-background px-3 py-2"
									>
										<Checkbox
											aria-label={`Assign ${endpoint.name}`}
											checked={endpointIds.includes(endpoint.id)}
											onCheckedChange={(checked) => {
												setEndpointIds((prev) =>
													checked
														? [...new Set([...prev, endpoint.id])]
														: prev.filter((id) => id !== endpoint.id),
												);
											}}
										/>
										<span>
											<span className="block text-sm font-medium">
												{endpoint.name}
											</span>
											<span className="block font-mono text-xs text-muted-foreground">
												{endpoint.id}
											</span>
										</span>
									</div>
								))}
							</div>
						)}
					</div>

					{submitted && error ? (
						<div className="xp-alert xp-alert-error">{error}</div>
					) : null}
					{!canWrite ? (
						<div className="xp-alert xp-alert-warning">
							Viewer role cannot create demo records. Switch role on login.
						</div>
					) : null}

					<div className="flex flex-wrap justify-end gap-2 border-t border-border/70 pt-4">
						<Button asChild variant="ghost">
							<Link to="/demo/users">Cancel</Link>
						</Button>
						<Button
							type="submit"
							loading={saving}
							disabled={!canWrite || saving}
						>
							Create user
						</Button>
					</div>
				</div>
			</form>
		</div>
	);
}

export function DemoUserDetailsPage() {
	const { userId } = useParams({ from: "/demo/users/$userId" });
	const { state, updateUser, deleteUser } = useDemo();
	const { pushToast } = useToast();
	const navigate = useNavigate();
	const [confirmOpen, setConfirmOpen] = useState(false);
	const user = state.users.find((item) => item.id === userId);
	const [selectedIds, setSelectedIds] = useState<string[]>(
		user?.endpointIds ?? [],
	);
	const [subscriptionFormat, setSubscriptionFormat] = useState<
		"raw" | "mihomo"
	>("raw");
	const [mihomoMixinYaml, setMihomoMixinYaml] = useState(
		user?.mihomoMixinYaml ?? "",
	);
	const canWrite = state.session?.role !== "viewer";

	useEffect(() => {
		setSelectedIds(user?.endpointIds ?? []);
		setMihomoMixinYaml(user?.mihomoMixinYaml ?? "");
	}, [user?.endpointIds, user?.mihomoMixinYaml]);

	if (!user) {
		return (
			<PageState
				variant="error"
				title="User not found"
				description="The selected demo user does not exist in this seed."
				action={
					<Link className={buttonVariants()} to="/demo/users">
						Back to users
					</Link>
				}
			/>
		);
	}

	const dirty = selectedIds.join("|") !== user.endpointIds.join("|");
	const mihomoDirty = mihomoMixinYaml !== user.mihomoMixinYaml;
	const assignedEndpoints = state.endpoints.filter((endpoint) =>
		user.endpointIds.includes(endpoint.id),
	);
	const subscriptionPreview =
		subscriptionFormat === "mihomo"
			? [
					"proxies:",
					...assignedEndpoints.map(
						(endpoint) =>
							`  - name: ${endpoint.name}\n    type: ${
								endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss"
							}\n    server: ${
								state.nodes.find((node) => node.id === endpoint.nodeId)
									?.accessHost ?? endpoint.nodeId
							}\n    port: ${endpoint.port}`,
					),
					user.mihomoMixinYaml.trim()
						? `# user mixin\n${user.mihomoMixinYaml.trim()}`
						: "# no user mixin",
				].join("\n")
			: assignedEndpoints.length > 0
				? assignedEndpoints
						.map(
							(endpoint) =>
								`${endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss"}://${user.subscriptionToken}@${state.nodes.find((node) => node.id === endpoint.nodeId)?.accessHost ?? endpoint.nodeId}:${endpoint.port}#${endpoint.name}`,
						)
						.join("\n")
				: "# no endpoint access assigned";

	return (
		<div className="space-y-6">
			<PageHeader
				title={user.displayName}
				description={user.email}
				meta={
					<>
						<Badge variant={userStatusVariant(user.status)}>
							{user.status}
						</Badge>
						<Badge variant="ghost" className="font-mono">
							{user.tier}
						</Badge>
						{dirty ? <Badge variant="warning">unsaved access</Badge> : null}
					</>
				}
				actions={
					<>
						<Button asChild variant="ghost" size="sm">
							<Link to="/demo/users">Back</Link>
						</Button>
						<Button
							variant="danger"
							size="sm"
							disabled={!canWrite}
							onClick={() => setConfirmOpen(true)}
						>
							Delete
						</Button>
					</>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Quota
					</p>
					<p className="mt-2 font-mono text-lg">
						{formatGb(user.quotaUsedGb)} / {formatGb(user.quotaLimitGb)}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Locale
					</p>
					<p className="mt-2 font-mono text-lg">{user.locale}</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Token
					</p>
					<p className="mt-2 truncate font-mono text-sm">
						{user.subscriptionToken}
					</p>
				</div>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<div className="flex flex-wrap items-start justify-between gap-3">
						<div>
							<h2 className="xp-card-title">Quota status</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Per-node mock enforcement follows the current quota policy.
							</p>
						</div>
						<Badge variant="ghost">{state.quotaPolicy.resetPolicy}</Badge>
					</div>
					<div className="xp-table-wrap">
						<table className="xp-table xp-table-zebra">
							<thead>
								<tr>
									<th>Node</th>
									<th>Access</th>
									<th>Remaining</th>
									<th>Policy weight</th>
									<th>Last seen</th>
								</tr>
							</thead>
							<tbody>
								{state.nodes.map((node) => {
									const endpointCount = state.endpoints.filter(
										(endpoint) =>
											endpoint.nodeId === node.id &&
											user.endpointIds.includes(endpoint.id),
									).length;
									const remaining =
										user.quotaLimitGb === null
											? null
											: Math.max(0, user.quotaLimitGb - user.quotaUsedGb);
									const blocked =
										state.quotaPolicy.enforcementMode === "block" &&
										user.status === "quota_limited" &&
										endpointCount > 0;
									return (
										<tr key={node.id}>
											<td>
												<p className="font-medium">{node.name}</p>
												<p className="font-mono text-xs text-muted-foreground">
													{node.id}
												</p>
											</td>
											<td>
												<Badge
													variant={
														blocked
															? "destructive"
															: endpointCount > 0
																? "success"
																: "ghost"
													}
												>
													{blocked
														? "blocked"
														: endpointCount > 0
															? `${endpointCount} endpoint(s)`
															: "no access"}
												</Badge>
											</td>
											<td className="font-mono text-xs">
												{formatGb(remaining)}
											</td>
											<td className="font-mono text-xs">
												{state.quotaPolicy.nodeWeights[node.id] ?? 0}
											</td>
											<td className="font-mono text-xs">
												{shortDate(node.lastSeenAt)}
											</td>
										</tr>
									);
								})}
							</tbody>
						</table>
					</div>
				</div>
			</section>

			<section className="xp-card">
				<div className="xp-card-body">
					<div className="flex flex-wrap items-start justify-between gap-3">
						<div>
							<h2 className="xp-card-title">Endpoint access</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Changing access updates the subscription preview immediately
								after save.
							</p>
						</div>
						<Button
							disabled={!dirty || !canWrite}
							onClick={() => {
								updateUser(user.id, { endpointIds: selectedIds });
								pushToast({ variant: "success", message: "Access saved." });
							}}
						>
							Save access
						</Button>
					</div>

					<div className="grid gap-2 md:grid-cols-2">
						{state.endpoints.map((endpoint) => (
							<div
								key={endpoint.id}
								className="flex items-start gap-3 rounded-xl border border-border/70 bg-muted/30 px-3 py-2"
							>
								<Checkbox
									aria-label={`Toggle access for ${endpoint.name}`}
									checked={selectedIds.includes(endpoint.id)}
									disabled={!canWrite}
									onCheckedChange={(checked) => {
										setSelectedIds((prev) =>
											checked
												? [...new Set([...prev, endpoint.id])]
												: prev.filter((id) => id !== endpoint.id),
										);
									}}
								/>
								<span>
									<span className="block text-sm font-medium">
										{endpoint.name}
									</span>
									<span className="block font-mono text-xs text-muted-foreground">
										{endpoint.id}
									</span>
								</span>
							</div>
						))}
					</div>
				</div>
			</section>

			<section className="xp-card">
				<div className="xp-card-body">
					<div className="flex flex-wrap items-start justify-between gap-3">
						<div>
							<h2 className="xp-card-title">Subscription result</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Format switch mirrors the live subscription preview workflow.
							</p>
						</div>
						<select
							className="xp-select w-40"
							value={subscriptionFormat}
							aria-label="Subscription format"
							onChange={(event) =>
								setSubscriptionFormat(event.target.value as "raw" | "mihomo")
							}
						>
							<option value="raw">Raw</option>
							<option value="mihomo">Mihomo</option>
						</select>
					</div>
					<div className="rounded-2xl border border-border/70 bg-muted/35 p-4">
						<pre className="whitespace-pre-wrap break-words font-mono text-xs">
							{subscriptionPreview}
						</pre>
						<div className="mt-3 flex flex-wrap gap-2">
							<CopyButton text={subscriptionPreview} label="Copy preview" />
							<CopyButton
								text={subscriptionUrl(user.subscriptionToken)}
								label="Copy URL"
							/>
							<Badge variant="ghost">
								{user.endpointIds.length} endpoint(s) in subscription
							</Badge>
						</div>
					</div>
				</div>
			</section>

			<section className="xp-card">
				<div className="xp-card-body">
					<div className="flex flex-wrap items-start justify-between gap-3">
						<div>
							<h2 className="xp-card-title">Mihomo profile</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Per-user mixin is stored in the mock user record and reflected
								in Mihomo preview output.
							</p>
						</div>
						<Button
							disabled={!mihomoDirty || !canWrite}
							onClick={() => {
								updateUser(user.id, { mihomoMixinYaml });
								pushToast({
									variant: "success",
									message: "Mihomo profile saved.",
								});
							}}
						>
							Save profile
						</Button>
					</div>
					<Textarea
						value={mihomoMixinYaml}
						onChange={(event) => setMihomoMixinYaml(event.target.value)}
						className="min-h-56 font-mono"
						aria-label="Mihomo mixin config"
						placeholder="rules:\n  - DOMAIN-SUFFIX,example.net,DIRECT"
						disabled={!canWrite}
					/>
					{mihomoDirty ? (
						<div className="xp-alert xp-alert-warning">
							Mihomo profile has unsaved changes.
						</div>
					) : null}
				</div>
			</section>

			<ConfirmDialog
				open={confirmOpen}
				title={`Delete ${user.displayName}?`}
				description="This removes the user from the mock state. You can undo from the users list."
				confirmLabel="Delete user"
				onCancel={() => setConfirmOpen(false)}
				onConfirm={() => {
					deleteUser(user.id);
					setConfirmOpen(false);
					pushToast({
						variant: "info",
						message: "User deleted. Undo is available.",
					});
					navigate({ to: "/demo/users" });
				}}
			/>
		</div>
	);
}
