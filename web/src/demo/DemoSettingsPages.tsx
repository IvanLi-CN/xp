import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";
import { formatGb, shortDate } from "./format";
import { useDemo } from "./store";
import type { DemoQuotaPolicy, DemoServiceConfig } from "./types";

const tiers = ["p1", "p2", "p3"] as const;

function parseOptionalNumber(value: string): number | null {
	if (value.trim() === "") return null;
	return Number(value);
}

function isValidHostname(value: string): boolean {
	return /^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/i.test(value.trim());
}

function configEquals(a: unknown, b: unknown): boolean {
	return JSON.stringify(a) === JSON.stringify(b);
}

export function DemoQuotaPolicyPage() {
	const { state, updateQuotaPolicy } = useDemo();
	const { pushToast } = useToast();
	const [defaultLimitGb, setDefaultLimitGb] = useState(
		state.quotaPolicy.defaultLimitGb?.toString() ?? "",
	);
	const [resetPolicy, setResetPolicy] = useState(state.quotaPolicy.resetPolicy);
	const [enforcementMode, setEnforcementMode] = useState(
		state.quotaPolicy.enforcementMode,
	);
	const [tierWeights, setTierWeights] = useState({
		...state.quotaPolicy.tierWeights,
	});
	const [nodeWeights, setNodeWeights] = useState({
		...state.quotaPolicy.nodeWeights,
	});

	useEffect(() => {
		setDefaultLimitGb(state.quotaPolicy.defaultLimitGb?.toString() ?? "");
		setResetPolicy(state.quotaPolicy.resetPolicy);
		setEnforcementMode(state.quotaPolicy.enforcementMode);
		setTierWeights({ ...state.quotaPolicy.tierWeights });
		setNodeWeights({ ...state.quotaPolicy.nodeWeights });
	}, [state.quotaPolicy]);

	const nextDefault = parseOptionalNumber(defaultLimitGb);
	const nextPolicy: DemoQuotaPolicy = {
		defaultLimitGb: nextDefault,
		resetPolicy,
		enforcementMode,
		tierWeights,
		nodeWeights,
	};
	const invalidNumber =
		(nextDefault !== null &&
			(!Number.isFinite(nextDefault) || nextDefault < 0)) ||
		tiers.some(
			(tier) => tierWeights[tier] < 0 || !Number.isFinite(tierWeights[tier]),
		) ||
		state.nodes.some((node) => {
			const value = nodeWeights[node.id] ?? 0;
			return value < 0 || !Number.isFinite(value);
		});
	const dirty = !configEquals(nextPolicy, state.quotaPolicy);
	const canWrite = state.session?.role !== "viewer";
	const quotaLimitedUsers = state.users.filter(
		(user) => user.status === "quota_limited",
	).length;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Quota policy"
				description="Tune the mock enforcement policy and inspect how it affects users and nodes."
				meta={dirty ? <Badge variant="warning">unsaved policy</Badge> : null}
				actions={
					<Button
						disabled={!dirty || invalidNumber || !canWrite}
						onClick={() => {
							updateQuotaPolicy(nextPolicy);
							pushToast({ variant: "success", message: "Quota policy saved." });
						}}
					>
						Save policy
					</Button>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Default limit
					</p>
					<p className="mt-2 font-mono text-lg">
						{formatGb(state.quotaPolicy.defaultLimitGb)}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Enforcement
					</p>
					<p className="mt-2 font-mono text-lg">
						{state.quotaPolicy.enforcementMode}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Blocked users
					</p>
					<p className="mt-2 font-mono text-lg">{quotaLimitedUsers}</p>
				</div>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Global rules</h2>
					<div className="grid gap-4 md:grid-cols-3">
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-quota-limit">
								Default quota GiB
							</label>
							<Input
								id="demo-quota-limit"
								value={defaultLimitGb}
								inputMode="numeric"
								placeholder="empty means unlimited"
								onChange={(event) => setDefaultLimitGb(event.target.value)}
							/>
						</div>
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-quota-reset">
								Reset policy
							</label>
							<select
								id="demo-quota-reset"
								className="xp-select"
								value={resetPolicy}
								onChange={(event) =>
									setResetPolicy(
										event.target.value as DemoQuotaPolicy["resetPolicy"],
									)
								}
							>
								<option value="never">Never</option>
								<option value="weekly">Weekly</option>
								<option value="monthly">Monthly</option>
							</select>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-quota-enforcement"
							>
								Enforcement mode
							</label>
							<select
								id="demo-quota-enforcement"
								className="xp-select"
								value={enforcementMode}
								onChange={(event) =>
									setEnforcementMode(
										event.target.value as DemoQuotaPolicy["enforcementMode"],
									)
								}
							>
								<option value="report">Report only</option>
								<option value="block">Block over quota</option>
							</select>
						</div>
					</div>
					{invalidNumber ? (
						<div className="xp-alert xp-alert-error">
							Quota numbers and weights must be non-negative numbers.
						</div>
					) : null}
				</div>
			</section>

			<div className="grid gap-6 xl:grid-cols-2">
				<section className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Priority weights</h2>
						<div className="space-y-3">
							{tiers.map((tier) => (
								<div
									key={tier}
									className="grid gap-3 rounded-xl border border-border/70 bg-muted/30 p-3 sm:grid-cols-[5rem_minmax(0,1fr)_8rem]"
								>
									<Badge variant="ghost" className="font-mono">
										{tier}
									</Badge>
									<p className="text-sm text-muted-foreground">
										{state.users.filter((user) => user.tier === tier).length}{" "}
										user(s) in this seed
									</p>
									<Input
										value={String(tierWeights[tier])}
										inputMode="numeric"
										aria-label={`${tier} weight`}
										onChange={(event) =>
											setTierWeights((prev) => ({
												...prev,
												[tier]: Number(event.target.value),
											}))
										}
									/>
								</div>
							))}
						</div>
					</div>
				</section>

				<section className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Node weights</h2>
						<div className="space-y-3">
							{state.nodes.map((node) => (
								<div
									key={node.id}
									className="grid gap-3 rounded-xl border border-border/70 bg-muted/30 p-3 sm:grid-cols-[minmax(0,1fr)_8rem]"
								>
									<div>
										<p className="font-medium">{node.name}</p>
										<p className="font-mono text-xs text-muted-foreground">
											{node.id}
										</p>
									</div>
									<Input
										value={String(nodeWeights[node.id] ?? 0)}
										inputMode="numeric"
										aria-label={`${node.name} weight`}
										onChange={(event) =>
											setNodeWeights((prev) => ({
												...prev,
												[node.id]: Number(event.target.value),
											}))
										}
									/>
								</div>
							))}
						</div>
					</div>
				</section>
			</div>
		</div>
	);
}

export function DemoRealityDomainsPage() {
	const {
		state,
		createRealityDomain,
		updateRealityDomain,
		deleteRealityDomain,
		moveRealityDomain,
	} = useDemo();
	const { pushToast } = useToast();
	const [hostname, setHostname] = useState("assets.cdn.microsoft.com");
	const [notes, setNotes] = useState("Fallback Reality camouflage domain.");
	const [nodeIds, setNodeIds] = useState<string[]>(
		[state.nodes[0]?.id ?? ""].filter(Boolean),
	);
	const [submitted, setSubmitted] = useState(false);
	const canWrite = state.session?.role !== "viewer";
	const duplicate = state.realityDomains.some(
		(domain) => domain.hostname === hostname.trim().toLowerCase(),
	);
	const formError = !isValidHostname(hostname)
		? "Hostname must be a valid domain."
		: duplicate
			? "This hostname already exists."
			: nodeIds.length === 0
				? "Select at least one node."
				: null;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Reality domains"
				description="Manage mock serverName inventory for Reality endpoints."
				actions={
					<Badge variant={canWrite ? "success" : "warning"}>
						{canWrite ? "editable" : "viewer is read-only"}
					</Badge>
				}
			/>

			<form
				className="xp-card"
				onSubmit={(event) => {
					event.preventDefault();
					setSubmitted(true);
					if (formError || !canWrite) return;
					const domain = createRealityDomain({
						hostname,
						enabled: true,
						nodeIds,
						notes,
					});
					setHostname("");
					setNotes("");
					pushToast({
						variant: "success",
						message: `Reality domain ${domain.hostname} added.`,
					});
				}}
			>
				<div className="xp-card-body">
					<h2 className="xp-card-title">Add domain</h2>
					<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(14rem,0.6fr)]">
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-domain-host">
								Hostname
							</label>
							<Input
								id="demo-domain-host"
								value={hostname}
								className="font-mono"
								onChange={(event) => setHostname(event.target.value)}
							/>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-domain-notes"
							>
								Notes
							</label>
							<Input
								id="demo-domain-notes"
								value={notes}
								onChange={(event) => setNotes(event.target.value)}
							/>
						</div>
					</div>
					<div className="grid gap-2 md:grid-cols-3">
						{state.nodes.map((node) => (
							<div
								key={node.id}
								className="flex items-center gap-3 rounded-xl border border-border/70 bg-muted/30 px-3 py-2"
							>
								<Checkbox
									aria-label={`Use ${node.name}`}
									checked={nodeIds.includes(node.id)}
									onCheckedChange={(checked) =>
										setNodeIds((prev) =>
											checked
												? [...new Set([...prev, node.id])]
												: prev.filter((id) => id !== node.id),
										)
									}
								/>
								<span className="text-sm">{node.name}</span>
							</div>
						))}
					</div>
					{submitted && formError ? (
						<div className="xp-alert xp-alert-error">{formError}</div>
					) : null}
					<div className="flex justify-end">
						<Button type="submit" disabled={!canWrite}>
							Add domain
						</Button>
					</div>
				</div>
			</form>

			{state.realityDomains.length === 0 ? (
				<PageState
					variant="empty"
					title="No Reality domains"
					description="Add a domain before creating Reality endpoints."
				/>
			) : (
				<div className="xp-table-wrap">
					<table className="xp-table xp-table-zebra">
						<thead>
							<tr>
								<th>Domain</th>
								<th>Status</th>
								<th>Nodes</th>
								<th>Last validation</th>
								<th>Actions</th>
							</tr>
						</thead>
						<tbody>
							{state.realityDomains.map((domain, index) => (
								<tr key={domain.id}>
									<td>
										<p className="font-mono text-sm">{domain.hostname}</p>
										<p className="text-xs text-muted-foreground">
											{domain.notes}
										</p>
									</td>
									<td>
										<div className="flex items-center gap-2">
											<Checkbox
												checked={domain.enabled}
												disabled={!canWrite}
												aria-label={`Toggle ${domain.hostname}`}
												onCheckedChange={(checked) =>
													updateRealityDomain(domain.id, {
														enabled: checked === true,
													})
												}
											/>
											<Badge variant={domain.enabled ? "success" : "ghost"}>
												{domain.enabled ? "enabled" : "disabled"}
											</Badge>
										</div>
									</td>
									<td className="text-sm">
										{domain.nodeIds
											.map(
												(nodeId) =>
													state.nodes.find((node) => node.id === nodeId)
														?.name ?? nodeId,
											)
											.join(", ")}
									</td>
									<td className="font-mono text-xs">
										{shortDate(domain.lastValidatedAt)}
									</td>
									<td>
										<div className="flex flex-wrap gap-2">
											<Button
												variant="secondary"
												size="sm"
												disabled={!canWrite || index === 0}
												aria-label={`Move ${domain.hostname} up`}
												onClick={() => moveRealityDomain(domain.id, "up")}
											>
												<Icon name="tabler:arrow-up" ariaLabel="Move up" />
											</Button>
											<Button
												variant="secondary"
												size="sm"
												disabled={
													!canWrite || index === state.realityDomains.length - 1
												}
												aria-label={`Move ${domain.hostname} down`}
												onClick={() => moveRealityDomain(domain.id, "down")}
											>
												<Icon name="tabler:arrow-down" ariaLabel="Move down" />
											</Button>
											<Button
												variant="danger"
												size="sm"
												disabled={!canWrite}
												onClick={() => deleteRealityDomain(domain.id)}
											>
												Delete
											</Button>
										</div>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			)}
		</div>
	);
}

export function DemoServiceConfigPage() {
	const { state, updateServiceConfig } = useDemo();
	const { pushToast } = useToast();
	const [draft, setDraft] = useState<DemoServiceConfig>(state.serviceConfig);
	const [saving, setSaving] = useState(false);
	const canWrite = state.session?.role !== "viewer";

	useEffect(() => {
		setDraft(state.serviceConfig);
	}, [state.serviceConfig]);

	const originError = (() => {
		try {
			const url = new URL(draft.publicOrigin);
			return url.protocol === "https:" ? null : "Public origin must use https.";
		} catch {
			return "Public origin must be a valid URL.";
		}
	})();
	const dirty = !configEquals(draft, state.serviceConfig);
	const firstToken = state.users[0]?.subscriptionToken ?? "sub_01HXPDEMO";
	const previewUrl = `${draft.publicOrigin.replace(/\/$/, "")}/sub/${firstToken}?format=${draft.defaultSubscriptionFormat}`;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Service config"
				description="Edit high-level mock delivery settings used by subscription output and operations."
				meta={dirty ? <Badge variant="warning">unsaved config</Badge> : null}
				actions={
					<Button
						loading={saving}
						disabled={!dirty || Boolean(originError) || !canWrite || saving}
						onClick={() => {
							setSaving(true);
							window.setTimeout(() => {
								updateServiceConfig(draft);
								setSaving(false);
								pushToast({
									variant: "success",
									message: "Service config saved.",
								});
							}, 500);
						}}
					>
						Save config
					</Button>
				}
			/>

			<section className="xp-card">
				<div className="xp-card-body">
					<div className="grid gap-4 md:grid-cols-2">
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-public-origin"
							>
								Public origin
							</label>
							<Input
								id="demo-public-origin"
								value={draft.publicOrigin}
								className="font-mono"
								onChange={(event) =>
									setDraft((prev) => ({
										...prev,
										publicOrigin: event.target.value,
									}))
								}
							/>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-subscription-format"
							>
								Default subscription format
							</label>
							<select
								id="demo-subscription-format"
								className="xp-select"
								value={draft.defaultSubscriptionFormat}
								onChange={(event) =>
									setDraft((prev) => ({
										...prev,
										defaultSubscriptionFormat: event.target
											.value as DemoServiceConfig["defaultSubscriptionFormat"],
									}))
								}
							>
								<option value="raw">Raw</option>
								<option value="mihomo">Mihomo</option>
							</select>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-mihomo-delivery"
							>
								Mihomo default delivery
							</label>
							<select
								id="demo-mihomo-delivery"
								className="xp-select"
								value={draft.mihomoDelivery}
								onChange={(event) =>
									setDraft((prev) => ({
										...prev,
										mihomoDelivery: event.target
											.value as DemoServiceConfig["mihomoDelivery"],
									}))
								}
							>
								<option value="inline">Inline proxies</option>
								<option value="provider">Proxy provider</option>
							</select>
						</div>
						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-restart">
								Xray restart strategy
							</label>
							<select
								id="demo-restart"
								className="xp-select"
								value={draft.xrayRestartStrategy}
								onChange={(event) =>
									setDraft((prev) => ({
										...prev,
										xrayRestartStrategy: event.target
											.value as DemoServiceConfig["xrayRestartStrategy"],
									}))
								}
							>
								<option value="rolling">Rolling</option>
								<option value="immediate">Immediate</option>
							</select>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-audit-retention"
							>
								Audit retention days
							</label>
							<Input
								id="demo-audit-retention"
								value={String(draft.auditLogRetentionDays)}
								inputMode="numeric"
								onChange={(event) =>
									setDraft((prev) => ({
										...prev,
										auditLogRetentionDays: Number(event.target.value),
									}))
								}
							/>
						</div>
					</div>
					{originError ? (
						<div className="xp-alert xp-alert-error">{originError}</div>
					) : null}
				</div>
			</section>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Subscription preview</h2>
					<div className="rounded-2xl border border-border/70 bg-muted/35 p-4">
						<p className="break-all font-mono text-sm">{previewUrl}</p>
						<div className="mt-3 flex flex-wrap gap-2">
							<CopyButton text={previewUrl} label="Copy preview URL" />
							<Badge variant="ghost">{draft.mihomoDelivery}</Badge>
							<Badge variant="ghost">{draft.xrayRestartStrategy}</Badge>
						</div>
					</div>
				</div>
			</section>
		</div>
	);
}

function redactMihomo(value: string): { output: string; count: number } {
	const patterns = [
		/sub_[A-Z0-9_]+/gi,
		/(password:\s*)(.+)/gi,
		/(uuid:\s*)([a-f0-9-]{24,})/gi,
		/(server:\s*)([^\n]+)/gi,
	];
	let count = 0;
	let output = value;
	for (const pattern of patterns) {
		output = output.replace(pattern, (...args: string[]) => {
			count += 1;
			if (args.length > 3) return `${args[1]}[REDACTED]`;
			return "[REDACTED]";
		});
	}
	return { output, count };
}

export function DemoToolsPage() {
	const { state, addToolRun } = useDemo();
	const { pushToast } = useToast();
	const [source, setSource] = useState(
		"proxies:\n  - name: tokyo-reality\n    server: tokyo-1.edge.example.net\n    uuid: 2f4c17c2-7b0f-47f5-b13a-7a8d2a139f0c\n    password: sub_01HXPDEMO0LINCHEN8ZPMDV\n",
	);
	const [output, setOutput] = useState("");
	const [running, setRunning] = useState(false);
	const canWrite = state.session?.role !== "viewer";

	return (
		<div className="space-y-6">
			<PageHeader
				title="Tools"
				description="Run safe mock operations that mirror admin utilities without touching a backend."
				meta={
					<Badge variant={canWrite ? "success" : "warning"}>mock only</Badge>
				}
				actions={
					<Button
						loading={running}
						disabled={!canWrite || running}
						onClick={() => {
							setRunning(true);
							window.setTimeout(() => {
								const result = redactMihomo(source);
								setOutput(result.output);
								const success = result.count > 0;
								addToolRun(
									"mihomo_redact",
									success ? "success" : "error",
									success
										? `Redacted ${result.count} sensitive field(s).`
										: "No redactable Mihomo fields were found.",
								);
								pushToast({
									variant: success ? "success" : "error",
									message: success
										? "Mihomo config redacted."
										: "Nothing was redacted.",
								});
								setRunning(false);
							}, 450);
						}}
					>
						Run redaction
					</Button>
				}
			/>

			<div className="grid gap-6 xl:grid-cols-2">
				<section className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Mihomo redact input</h2>
						<Textarea
							value={source}
							onChange={(event) => setSource(event.target.value)}
							className="min-h-72 font-mono"
							aria-label="Mihomo redact input"
						/>
					</div>
				</section>
				<section className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Output</h2>
						{output ? (
							<div className="rounded-2xl border border-border/70 bg-muted/35 p-4">
								<pre className="whitespace-pre-wrap break-words font-mono text-xs">
									{output}
								</pre>
								<div className="mt-3">
									<CopyButton text={output} label="Copy output" />
								</div>
							</div>
						) : (
							<PageState
								variant="empty"
								title="No output yet"
								description="Run redaction to produce a safe config preview."
							/>
						)}
					</div>
				</section>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Tool history</h2>
					{state.toolRuns.length === 0 ? (
						<PageState
							variant="empty"
							title="No tool runs"
							description="Tool runs appear here after a mock operation."
						/>
					) : (
						<div className="xp-table-wrap">
							<table className="xp-table xp-table-zebra">
								<thead>
									<tr>
										<th>Run</th>
										<th>Kind</th>
										<th>Status</th>
										<th>Message</th>
									</tr>
								</thead>
								<tbody>
									{state.toolRuns.map((run) => (
										<tr key={run.id}>
											<td className="font-mono text-xs">{shortDate(run.at)}</td>
											<td className="font-mono text-xs">{run.kind}</td>
											<td>
												<Badge
													variant={
														run.status === "success" ? "success" : "destructive"
													}
												>
													{run.status}
												</Badge>
											</td>
											<td className="text-sm">{run.message}</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					)}
				</div>
			</section>
		</div>
	);
}
