import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import type { SubscriptionFormat } from "@/api/subscription";
import { Badge } from "@/components/ui/badge";

import {
	AccessMatrix,
	type AccessMatrixCellState,
} from "../components/AccessMatrix";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { CopyButton } from "../components/CopyButton";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { SubscriptionPreviewDialog } from "../components/SubscriptionPreviewDialog";
import { useToast } from "../components/Toast";
import { YamlCodeEditor } from "../components/YamlCodeEditor";
import { buttonVariants } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import {
	formatGb,
	formatPercent,
	shortDate,
	subscriptionUrl,
	userStatusVariant,
} from "./format";
import { useDemo } from "./store";
import type { DemoEndpoint, DemoUser } from "./types";

const DEMO_PROTOCOLS = [
	{ protocolId: "vless", label: "VLESS" },
	{ protocolId: "ss2022", label: "SS2022" },
] as const;

type DemoUserTab = "user" | "access" | "quotaStatus" | "usageDetails";

function endpointProtocolId(endpoint: DemoEndpoint) {
	return endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss2022";
}

function normalizeSelection(ids: string[]) {
	return [...new Set(ids)].sort();
}

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
	const user = state.users.find((item) => item.id === userId);
	const [tab, setTab] = useState<DemoUserTab>("user");
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [resetTokenOpen, setResetTokenOpen] = useState(false);
	const [resetCredentialsOpen, setResetCredentialsOpen] = useState(false);
	const [displayName, setDisplayName] = useState(user?.displayName ?? "");
	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		user?.quotaLimitGb === null ? "unlimited" : "monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState(0);
	const [tier, setTier] = useState<DemoUser["tier"]>(user?.tier ?? "p2");
	const [locale, setLocale] = useState(user?.locale ?? "en-US");
	const [selectedIds, setSelectedIds] = useState<string[]>(
		user?.endpointIds ?? [],
	);
	const [subscriptionFormat, setSubscriptionFormat] =
		useState<SubscriptionFormat>("raw");
	const [subscriptionOpen, setSubscriptionOpen] = useState(false);
	const [subscriptionLoading, setSubscriptionLoading] = useState(false);
	const [subscriptionText, setSubscriptionText] = useState("");
	const [subscriptionError, setSubscriptionError] = useState<string | null>(
		null,
	);
	const [mihomoMixinYaml, setMihomoMixinYaml] = useState(
		user?.mihomoMixinYaml ?? "",
	);
	const [mihomoExtraProxiesYaml, setMihomoExtraProxiesYaml] = useState("");
	const [mihomoExtraProxyProvidersYaml, setMihomoExtraProxyProvidersYaml] =
		useState("");
	const [activeUsageNodeId, setActiveUsageNodeId] = useState<string | null>(
		null,
	);
	const canWrite = state.session?.role !== "viewer";

	useEffect(() => {
		setDisplayName(user?.displayName ?? "");
		setResetPolicy(user?.quotaLimitGb === null ? "unlimited" : "monthly");
		setResetDay(1);
		setResetTzOffsetMinutes(0);
		setTier(user?.tier ?? "p2");
		setLocale(user?.locale ?? "en-US");
		setSelectedIds(user?.endpointIds ?? []);
		setMihomoMixinYaml(user?.mihomoMixinYaml ?? "");
		setMihomoExtraProxiesYaml("");
		setMihomoExtraProxyProvidersYaml("");
		setActiveUsageNodeId(null);
	}, [
		user?.displayName,
		user?.endpointIds,
		user?.locale,
		user?.mihomoMixinYaml,
		user?.quotaLimitGb,
		user?.tier,
	]);

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

	const currentUser = user;
	const assignedEndpoints = state.endpoints.filter((endpoint) =>
		currentUser.endpointIds.includes(endpoint.id),
	);
	const selectedEndpointSet = new Set(selectedIds);
	const dirty =
		normalizeSelection(selectedIds).join("|") !==
		normalizeSelection(currentUser.endpointIds).join("|");
	const profileDirty =
		displayName !== currentUser.displayName ||
		locale !== currentUser.locale ||
		tier !== currentUser.tier ||
		(resetPolicy === "unlimited") !== (currentUser.quotaLimitGb === null);
	const mihomoDirty = mihomoMixinYaml !== currentUser.mihomoMixinYaml;

	function endpointIdsFor(nodeId: string, protocolId: string) {
		return state.endpoints
			.filter(
				(endpoint) =>
					endpoint.nodeId === nodeId &&
					endpointProtocolId(endpoint) === protocolId,
			)
			.map((endpoint) => endpoint.id);
	}

	function setEndpointMembership(endpointIds: string[], checked: boolean) {
		setSelectedIds((prev) => {
			const next = new Set(prev);
			for (const endpointId of endpointIds) {
				if (checked) next.add(endpointId);
				else next.delete(endpointId);
			}
			return [...next];
		});
	}

	function toggleCell(nodeId: string, protocolId: string) {
		const endpointIds = endpointIdsFor(nodeId, protocolId);
		const allSelected = endpointIds.every((endpointId) =>
			selectedEndpointSet.has(endpointId),
		);
		setEndpointMembership(endpointIds, !allSelected);
	}

	function toggleRow(nodeId: string) {
		const endpointIds = state.endpoints
			.filter((endpoint) => endpoint.nodeId === nodeId)
			.map((endpoint) => endpoint.id);
		const allSelected = endpointIds.every((endpointId) =>
			selectedEndpointSet.has(endpointId),
		);
		setEndpointMembership(endpointIds, !allSelected);
	}

	function toggleColumn(protocolId: string) {
		const endpointIds = state.endpoints
			.filter((endpoint) => endpointProtocolId(endpoint) === protocolId)
			.map((endpoint) => endpoint.id);
		const allSelected = endpointIds.every((endpointId) =>
			selectedEndpointSet.has(endpointId),
		);
		setEndpointMembership(endpointIds, !allSelected);
	}

	function toggleAll() {
		const endpointIds = state.endpoints.map((endpoint) => endpoint.id);
		const allSelected = endpointIds.every((endpointId) =>
			selectedEndpointSet.has(endpointId),
		);
		setEndpointMembership(endpointIds, !allSelected);
	}

	const accessCells = useMemo<
		Record<string, Record<string, AccessMatrixCellState>>
	>(() => {
		const selectedForCells = new Set(selectedIds);
		const next: Record<string, Record<string, AccessMatrixCellState>> = {};
		for (const node of state.nodes) {
			next[node.id] = {};
			for (const protocol of DEMO_PROTOCOLS) {
				const options = state.endpoints
					.filter(
						(endpoint) =>
							endpoint.nodeId === node.id &&
							endpointProtocolId(endpoint) === protocol.protocolId,
					)
					.map((endpoint) => ({
						endpointId: endpoint.id,
						tag: endpoint.name,
						port: endpoint.port,
					}));
				const selectedEndpointIds = options
					.map((option) => option.endpointId)
					.filter((endpointId) => selectedForCells.has(endpointId));
				next[node.id][protocol.protocolId] =
					options.length === 0
						? {
								value: "disabled",
								reason: "No endpoint for this node/protocol",
							}
						: {
								value: selectedEndpointIds.length > 0 ? "on" : "off",
								meta: {
									options,
									selectedEndpointIds,
								},
							};
			}
		}
		return next;
	}, [selectedIds, state.endpoints, state.nodes]);

	const usageGroups = state.nodes
		.map((node) => ({
			node,
			endpoints: assignedEndpoints.filter(
				(endpoint) => endpoint.nodeId === node.id,
			),
		}))
		.filter((group) => group.endpoints.length > 0);
	const activeUsageGroup =
		usageGroups.find((group) => group.node.id === activeUsageNodeId) ??
		usageGroups[0] ??
		null;

	function buildSubscriptionPreview(format: SubscriptionFormat) {
		if (assignedEndpoints.length === 0) return "# no endpoint access assigned";
		if (
			format === "clash" ||
			format === "mihomo" ||
			format === "mihomo_legacy" ||
			format === "mihomo_provider"
		) {
			const providerComment =
				format === "mihomo_provider"
					? "# provider mode preview"
					: format === "mihomo_legacy"
						? "# legacy mihomo preview"
						: format === "clash"
							? "# clash-compatible preview"
							: "# default mihomo preview";
			return [
				providerComment,
				"proxies:",
				...assignedEndpoints.map((endpoint) => {
					const node = state.nodes.find((item) => item.id === endpoint.nodeId);
					return `  - name: ${endpoint.name}\n    type: ${
						endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss"
					}\n    server: ${node?.accessHost ?? endpoint.nodeId}\n    port: ${
						endpoint.port
					}`;
				}),
				currentUser.mihomoMixinYaml.trim()
					? `# user mixin\n${currentUser.mihomoMixinYaml.trim()}`
					: "# no user mixin",
			].join("\n");
		}

		return assignedEndpoints
			.map((endpoint) => {
				const node = state.nodes.find((item) => item.id === endpoint.nodeId);
				return `${
					endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss"
				}://${currentUser.subscriptionToken}@${
					node?.accessHost ?? endpoint.nodeId
				}:${endpoint.port}#${endpoint.name}`;
			})
			.join("\n");
	}

	function fetchSubscriptionPreview() {
		setSubscriptionOpen(true);
		setSubscriptionLoading(true);
		setSubscriptionError(null);
		window.setTimeout(() => {
			setSubscriptionText(buildSubscriptionPreview(subscriptionFormat));
			setSubscriptionLoading(false);
		}, 240);
	}

	function resetSubscriptionToken() {
		const suffix = Date.now().toString(36).toUpperCase();
		updateUser(currentUser.id, {
			subscriptionToken: `sub_${currentUser.id.replace(/[^a-z0-9]/gi, "").toUpperCase()}_${suffix}`,
		});
		setResetTokenOpen(false);
		pushToast({ variant: "success", message: "Subscription token reset." });
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title={user.displayName}
				description="Manage profile, access, quota status, and usage details"
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
					<div className="flex flex-wrap items-center gap-2">
						<Button
							variant="ghost"
							size="sm"
							disabled={!canWrite}
							onClick={() => setResetTokenOpen(true)}
						>
							Reset token
						</Button>
						<Button
							variant="ghost"
							size="sm"
							disabled={!canWrite}
							onClick={() => setResetCredentialsOpen(true)}
						>
							Reset credentials
						</Button>
						<Button
							variant="danger"
							size="sm"
							disabled={!canWrite}
							onClick={() => setDeleteOpen(true)}
						>
							Delete user
						</Button>
					</div>
				}
			/>

			<div className="overflow-x-auto">
				<div className="inline-flex min-w-max items-center gap-1 rounded-2xl border border-border/70 bg-card p-1 shadow-sm">
					{(["user", "access", "quotaStatus", "usageDetails"] as const).map(
						(item) => (
							<Button
								key={item}
								type="button"
								size="sm"
								variant={tab === item ? "primary" : "ghost"}
								onClick={() => setTab(item)}
							>
								{item === "user"
									? "User"
									: item === "access"
										? "Access"
										: item === "quotaStatus"
											? "Quota status"
											: "Usage details"}
							</Button>
						),
					)}
				</div>
			</div>

			{tab === "user" ? (
				<div className="space-y-6">
					<div className="xp-card p-4 space-y-3">
						<div className="xp-field-stack gap-2">
							<span className="text-sm font-medium">Display name</span>
							<Input
								aria-label="Display name"
								value={displayName}
								onChange={(event) => setDisplayName(event.target.value)}
							/>
						</div>

						<div className="grid gap-3 md:grid-cols-3">
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Quota reset policy</span>
								<Select
									value={resetPolicy}
									onValueChange={(value) =>
										setResetPolicy(value as "monthly" | "unlimited")
									}
								>
									<SelectTrigger aria-label="Quota reset policy">
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										<SelectItem value="monthly">monthly</SelectItem>
										<SelectItem value="unlimited">unlimited</SelectItem>
									</SelectContent>
								</Select>
							</div>
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Day of month</span>
								<Input
									type="number"
									min={1}
									max={31}
									disabled={resetPolicy !== "monthly"}
									value={resetDay}
									onChange={(event) =>
										setResetDay(Number(event.target.value || "1"))
									}
								/>
							</div>
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">TZ offset (minutes)</span>
								<Input
									type="number"
									value={resetTzOffsetMinutes}
									onChange={(event) =>
										setResetTzOffsetMinutes(Number(event.target.value || "0"))
									}
								/>
							</div>
						</div>

						<div className="grid gap-3 md:grid-cols-2">
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Tier</span>
								<Select
									value={tier}
									onValueChange={(value) => setTier(value as DemoUser["tier"])}
								>
									<SelectTrigger aria-label="Tier">
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										<SelectItem value="p1">p1</SelectItem>
										<SelectItem value="p2">p2</SelectItem>
										<SelectItem value="p3">p3</SelectItem>
									</SelectContent>
								</Select>
							</div>
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Locale</span>
								<Input
									aria-label="Locale"
									value={locale}
									onChange={(event) => setLocale(event.target.value)}
								/>
							</div>
						</div>

						<div className="flex items-center gap-3 text-sm">
							<span className="font-medium">User ID:</span>
							<span className="font-mono">{user.id}</span>
						</div>
						<div className="flex items-center gap-3 text-sm">
							<span className="font-medium">Subscription token:</span>
							<span className="font-mono break-all">
								{user.subscriptionToken}
							</span>
						</div>

						<div className="rounded-2xl border border-border/70 p-3 space-y-3">
							<div className="flex flex-wrap items-end gap-3">
								<div className="xp-field-stack gap-2">
									<span className="text-sm font-medium">
										Subscription format
									</span>
									<Select
										value={subscriptionFormat}
										onValueChange={(value) =>
											setSubscriptionFormat(value as SubscriptionFormat)
										}
									>
										<SelectTrigger
											aria-label="Subscription format"
											data-testid="demo-subscription-format"
										>
											<SelectValue />
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="raw">raw</SelectItem>
											<SelectItem value="clash">clash</SelectItem>
											<SelectItem value="mihomo">mihomo(default)</SelectItem>
											<SelectItem value="mihomo_legacy">
												mihomo(legacy)
											</SelectItem>
											<SelectItem value="mihomo_provider">
												mihomo(provider)
											</SelectItem>
										</SelectContent>
									</Select>
								</div>
								<CopyButton
									text={subscriptionUrl(user.subscriptionToken)}
									label="Copy URL"
									ariaLabel="Copy subscription URL"
									className="self-end"
								/>
								<Button
									className="self-end"
									loading={subscriptionLoading}
									onClick={fetchSubscriptionPreview}
								>
									Fetch
								</Button>
							</div>
							<div className="text-xs text-muted-foreground">
								Preview opens in a modal. The mock output follows the current
								Demo seed and selected endpoint access.
							</div>
						</div>

						<div className="rounded-2xl border border-border/70 p-3 space-y-3">
							<div className="font-medium text-sm">
								Mihomo mixin config (per user)
							</div>
							<YamlCodeEditor
								label="mixin_yaml"
								value={mihomoMixinYaml}
								onChange={setMihomoMixinYaml}
								placeholder="Paste Mihomo mixin YAML"
								minRows={10}
							/>
							<YamlCodeEditor
								label="extra_proxies_yaml"
								value={mihomoExtraProxiesYaml}
								onChange={setMihomoExtraProxiesYaml}
								placeholder="- name: custom-ss\n  type: ss\n  ..."
								minRows={6}
							/>
							<YamlCodeEditor
								label="extra_proxy_providers_yaml"
								value={mihomoExtraProxyProvidersYaml}
								onChange={setMihomoExtraProxyProvidersYaml}
								placeholder="ProviderA:\n  type: http\n  ..."
								minRows={6}
							/>
							{mihomoDirty ? (
								<div className="rounded-xl border border-warning/30 bg-warning/10 px-4 py-2 text-sm">
									Mihomo profile has unsaved changes.
								</div>
							) : null}
							<div>
								<Button
									disabled={!canWrite || !mihomoDirty}
									onClick={() => {
										updateUser(user.id, { mihomoMixinYaml });
										pushToast({
											variant: "success",
											message: "Mihomo profile saved.",
										});
									}}
								>
									Save mihomo mixin
								</Button>
							</div>
						</div>

						<Button
							disabled={!canWrite || !profileDirty}
							onClick={() => {
								updateUser(user.id, {
									displayName,
									locale,
									tier,
									quotaLimitGb:
										resetPolicy === "unlimited"
											? null
											: (user.quotaLimitGb ??
												state.quotaPolicy.defaultLimitGb ??
												100),
								});
								pushToast({
									variant: "success",
									message: "User saved.",
								});
							}}
						>
							Save user
						</Button>
					</div>
				</div>
			) : null}

			{tab === "access" ? (
				<div className="space-y-4">
					<div className="flex flex-wrap items-center justify-between gap-3">
						<div className="text-sm opacity-70">
							Selected endpoints: {selectedIds.length}
						</div>
						<Button
							disabled={!dirty || !canWrite}
							onClick={() => {
								updateUser(user.id, { endpointIds: selectedIds });
								pushToast({ variant: "success", message: "Access saved." });
							}}
						>
							Apply access
						</Button>
					</div>
					{dirty ? (
						<output
							aria-live="polite"
							className="flex items-start gap-2 rounded-lg border border-border/70 bg-muted/30 px-3 py-2 text-xs leading-5 text-muted-foreground"
						>
							<Icon
								name="tabler:info-circle"
								className="mt-0.5 size-4 shrink-0 text-primary"
							/>
							<p>Access changes are local until Apply access is clicked.</p>
						</output>
					) : null}
					<AccessMatrix
						nodes={state.nodes.map((node) => ({
							nodeId: node.id,
							label: node.name,
							details: (
								<div className="space-y-0.5">
									<div className="text-xs text-muted-foreground">
										Remaining:{" "}
										{node.quotaLimitGb === null
											? "unlimited"
											: formatGb(
													Math.max(0, node.quotaLimitGb - node.quotaUsedGb),
												)}
									</div>
								</div>
							),
						}))}
						protocols={[...DEMO_PROTOCOLS]}
						cells={accessCells}
						disabled={!canWrite}
						onToggleCell={toggleCell}
						onToggleRow={toggleRow}
						onToggleColumn={toggleColumn}
						onToggleAll={toggleAll}
						onToggleCellEndpoint={(nodeId, protocolId, endpointId, checked) => {
							if (!endpointIdsFor(nodeId, protocolId).includes(endpointId)) {
								return;
							}
							setEndpointMembership([endpointId], checked);
						}}
					/>
				</div>
			) : null}

			{tab === "quotaStatus" ? (
				<div className="xp-card p-4 space-y-3">
					{state.quotaPolicy.enforcementMode === "block" &&
					user.status === "quota_limited" ? (
						<div className="rounded-xl border border-warning/30 bg-warning/10 px-4 py-2 text-sm">
							Quota status is partial for access decisions because this user is
							quota-limited.
						</div>
					) : null}
					{state.nodes.map((node) => {
						const endpointCount = state.endpoints.filter(
							(endpoint) =>
								endpoint.nodeId === node.id &&
								user.endpointIds.includes(endpoint.id),
						).length;
						const quotaLimitGb = user.quotaLimitGb;
						const remaining =
							quotaLimitGb === null
								? null
								: Math.max(0, quotaLimitGb - user.quotaUsedGb);
						return (
							<div
								key={node.id}
								className="rounded-2xl border border-border/70 p-3 space-y-1"
							>
								<div className="flex flex-wrap items-center justify-between gap-2">
									<div className="font-medium">{node.id}</div>
									<Badge variant={endpointCount > 0 ? "success" : "ghost"}>
										{endpointCount > 0
											? `${endpointCount} endpoint(s)`
											: "no access"}
									</Badge>
								</div>
								<div className="text-sm">
									Used {formatGb(user.quotaUsedGb)} /{" "}
									{formatGb(user.quotaLimitGb)}
								</div>
								<div className="text-sm opacity-70">
									Remaining: {formatGb(remaining)}
								</div>
							</div>
						);
					})}
				</div>
			) : null}

			{tab === "usageDetails" ? (
				<div className="space-y-4">
					{usageGroups.length > 0 ? (
						<>
							<div className="overflow-x-auto">
								<div
									className="inline-flex min-w-max items-center gap-1 rounded-2xl border border-border/70 bg-card p-1 shadow-sm"
									role="tablist"
									aria-label="Usage detail nodes"
								>
									{usageGroups.map((group) => {
										const selected =
											group.node.id === activeUsageGroup?.node.id;
										return (
											<Button
												key={group.node.id}
												type="button"
												size="sm"
												variant={selected ? "primary" : "ghost"}
												role="tab"
												aria-selected={selected}
												title={`${group.node.name} · ${group.node.id}`}
												onClick={() => setActiveUsageNodeId(group.node.id)}
											>
												{group.node.name}
											</Button>
										);
									})}
								</div>
							</div>
							{activeUsageGroup ? (
								<div className="xp-card p-4 space-y-4">
									<div>
										<h2 className="xp-card-title">
											Usage details · {activeUsageGroup.node.name}
										</h2>
										<p className="mt-1 text-sm text-muted-foreground">
											{activeUsageGroup.node.id} ·{" "}
											{activeUsageGroup.node.accessHost}
										</p>
									</div>
									<div className="xp-table-wrap">
										<table className="xp-table xp-table-zebra">
											<thead>
												<tr>
													<th>Endpoint</th>
													<th>Window</th>
													<th>Inbound IPs</th>
													<th>Transfer</th>
													<th>Last probe</th>
												</tr>
											</thead>
											<tbody>
												{activeUsageGroup.endpoints.map((endpoint, index) => (
													<tr key={endpoint.id}>
														<td>
															<p className="font-medium">{endpoint.name}</p>
															<p className="font-mono text-xs text-muted-foreground">
																{endpoint.id}
															</p>
														</td>
														<td className="font-mono text-xs">24h</td>
														<td className="font-mono text-xs">
															{3 + index * 2}
														</td>
														<td className="font-mono text-xs">
															{formatGb(Math.round(user.quotaUsedGb / 3))}
														</td>
														<td className="font-mono text-xs">
															{shortDate(endpoint.lastProbeAt)}
														</td>
													</tr>
												))}
											</tbody>
										</table>
									</div>
								</div>
							) : null}
						</>
					) : (
						<PageState
							variant="empty"
							title="No usage groups"
							description="This user has no active node memberships to aggregate inbound IP usage from."
						/>
					)}
				</div>
			) : null}

			<ConfirmDialog
				open={resetTokenOpen}
				title="Reset subscription token"
				description="This invalidates the old token in the mock state immediately."
				confirmLabel="Reset"
				onCancel={() => setResetTokenOpen(false)}
				onConfirm={resetSubscriptionToken}
			/>

			<ConfirmDialog
				open={resetCredentialsOpen}
				title="Reset credentials"
				description="This rotates derived credentials for the user in the demo flow."
				confirmLabel="Reset"
				onCancel={() => setResetCredentialsOpen(false)}
				onConfirm={() => {
					setResetCredentialsOpen(false);
					pushToast({ variant: "success", message: "Credentials reset." });
				}}
			/>

			<ConfirmDialog
				open={deleteOpen}
				title="Delete user"
				description="This removes the user from the mock state. You can undo from the users list."
				confirmLabel="Delete user"
				onCancel={() => setDeleteOpen(false)}
				onConfirm={() => {
					deleteUser(user.id);
					setDeleteOpen(false);
					pushToast({
						variant: "info",
						message: "User deleted. Undo is available.",
					});
					navigate({ to: "/demo/users" });
				}}
			/>

			<SubscriptionPreviewDialog
				open={subscriptionOpen}
				onClose={() => setSubscriptionOpen(false)}
				subscriptionUrl={subscriptionUrl(user.subscriptionToken)}
				format={subscriptionFormat}
				loading={subscriptionLoading}
				content={subscriptionText}
				error={subscriptionError}
			/>

			<div className="text-xs text-muted-foreground">
				Tip: use the Access tab to control endpoint membership directly in
				user/node/endpoint mode.
			</div>
			<Button asChild variant="ghost" size="sm">
				<Link to="/demo/users">Back to users</Link>
			</Button>
		</div>
	);
}
