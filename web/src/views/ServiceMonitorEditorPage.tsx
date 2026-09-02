import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { type AdminNode, fetchAdminNodes } from "../api/adminNodes";
import {
	type AdminServiceMonitor,
	type ObserverPolicy,
	ObserverPolicySchema,
	type ServiceMonitorKind,
	type ServiceMonitorTarget,
	createAdminMonitorDraftTest,
	createAdminServiceMonitor,
	fetchAdminMonitorDraftTest,
	fetchAdminServiceMonitor,
	monitorKind,
	patchAdminServiceMonitor,
} from "../api/adminServiceMonitors";
import { isBackendApiError } from "../api/backendError";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Label } from "../components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "../components/ui/tabs";

type FormState = {
	name: string;
	kind: ServiceMonitorKind;
	url: string;
	host: string;
	port: string;
	method: "get" | "head";
	bodyContains: string;
	intervalSeconds: "60" | "300" | "900" | "3600";
	observerPolicy: ObserverPolicy;
};

const INITIAL_FORM: FormState = {
	name: "",
	kind: "https",
	url: "https://example.com/health",
	host: "example.com",
	port: "443",
	method: "get",
	bodyContains: "",
	intervalSeconds: "60",
	observerPolicy: { mode: "exclude", node_ids: [] },
};

function errorMessage(error: unknown): string {
	if (isBackendApiError(error)) return `${error.code}: ${error.message}`;
	return error instanceof Error ? error.message : String(error);
}

function formFromMonitor(monitor: AdminServiceMonitor): FormState {
	const kind = monitorKind(monitor.target);
	const target = monitor.target;
	const http =
		target.kind === "http" || target.kind === "https" ? target : undefined;
	return {
		name: monitor.name,
		kind,
		url: http?.url ?? "",
		host: target.kind === "ping" || target.kind === "tcping" ? target.host : "",
		port: target.kind === "tcping" ? String(target.port) : "443",
		method: http?.method ?? "get",
		bodyContains: http?.body_contains ?? "",
		intervalSeconds: String(
			monitor.interval_seconds,
		) as FormState["intervalSeconds"],
		observerPolicy: ObserverPolicySchema.parse(monitor.observer_policy),
	};
}

function targetFromForm(form: FormState): ServiceMonitorTarget {
	if (form.kind === "ping") return { kind: "ping", host: form.host.trim() };
	if (form.kind === "tcping")
		return { kind: "tcping", host: form.host.trim(), port: Number(form.port) };
	const value = {
		url: form.url.trim(),
		method: form.method,
		accepted_statuses: [{ start: 200, end: 399 }],
		...(form.method === "get" && form.bodyContains.trim()
			? { body_contains: form.bodyContains.trim() }
			: {}),
	};
	return form.kind === "http"
		? { kind: "http", ...value }
		: { kind: "https", ...value };
}

function terminalDraftState(state: string | undefined): boolean {
	return (
		state === "succeeded" ||
		state === "failed" ||
		state === "unsupported" ||
		state === "interrupted"
	);
}

function observerStatusLabel(state: string): string {
	return state === "succeeded"
		? "Reached"
		: state === "running"
			? "Testing"
			: state === "queued"
				? "Queued"
				: state[0].toUpperCase() + state.slice(1);
}

function ServiceMonitorEditor({ monitorId }: { monitorId?: string }) {
	const editing = Boolean(monitorId);
	const adminToken = readAdminToken();
	const capability = useApiCapability("admin.service-monitors");
	const policyCapability = useApiCapability(
		"admin.service-monitor-observer-policy-v1",
	);
	const draftCapability = useApiCapability(
		"admin.service-monitor-draft-tests-v1",
	);
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const [form, setForm] = useState<FormState>(INITIAL_FORM);
	const [draftRunId, setDraftRunId] = useState(
		() => new URLSearchParams(window.location.search).get("draft_run") ?? "",
	);
	const [draft, setDraft] = useState<Awaited<
		ReturnType<typeof fetchAdminMonitorDraftTest>
	> | null>(null);
	const [draftInterrupted, setDraftInterrupted] = useState(false);

	const monitorQuery = useQuery({
		queryKey: ["adminServiceMonitor", adminToken, monitorId],
		enabled:
			Boolean(monitorId) && adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminServiceMonitor(adminToken, monitorId ?? "", signal),
	});
	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});
	const draftQuery = useQuery({
		queryKey: ["adminMonitorDraftTest", adminToken, draftRunId],
		enabled: adminToken.length > 0 && Boolean(draftRunId),
		queryFn: ({ signal }) =>
			fetchAdminMonitorDraftTest(adminToken, draftRunId, signal),
		refetchInterval: (query) =>
			terminalDraftState(query.state.data?.state) ? false : 500,
	});

	useEffect(() => {
		if (monitorQuery.data) setForm(formFromMonitor(monitorQuery.data));
	}, [monitorQuery.data]);
	useEffect(() => {
		if (draftQuery.data) {
			setDraft(draftQuery.data);
			setDraftInterrupted(false);
		}
	}, [draftQuery.data]);
	useEffect(() => {
		if (
			draftRunId &&
			draftQuery.isError &&
			isBackendApiError(draftQuery.error) &&
			draftQuery.error.status === 404
		) {
			setDraft(null);
			setDraftInterrupted(true);
		}
	}, [draftQuery.error, draftQuery.isError, draftRunId]);

	const testMutation = useMutation({
		mutationFn: () =>
			createAdminMonitorDraftTest(
				adminToken,
				targetFromForm(form),
				form.observerPolicy,
			),
		onSuccess: (run) => {
			setDraft(run);
			setDraftInterrupted(false);
			setDraftRunId(run.run_id);
			const search = new URLSearchParams(window.location.search);
			search.set("draft_run", run.run_id);
			search.set("draft_coordinator", run.coordinator_node_id);
			window.history.replaceState(
				{},
				"",
				`${window.location.pathname}?${search}`,
			);
		},
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});

	const saveMutation = useMutation({
		mutationFn: async () => {
			const legacyObserverPayload =
				form.observerPolicy.mode === "exclude" &&
				form.observerPolicy.node_ids.length === 0
					? { observer_node_ids: null }
					: form.observerPolicy.mode === "include"
						? { observer_node_ids: form.observerPolicy.node_ids }
						: {};
			const payload = {
				name: form.name.trim(),
				target: targetFromForm(form),
				interval_seconds: Number(form.intervalSeconds),
				...(policyCapability.available
					? { observer_policy: form.observerPolicy }
					: legacyObserverPayload),
			};
			if (!editing) return createAdminServiceMonitor(adminToken, payload);
			if (!monitorQuery.data) throw new Error("Monitor is not loaded.");
			return patchAdminServiceMonitor(
				adminToken,
				monitorQuery.data.monitor_id,
				{ ...payload, expected_revision: monitorQuery.data.revision },
			);
		},
		onSuccess: (monitor) => {
			queryClient.invalidateQueries({
				queryKey: ["adminServiceMonitors", adminToken],
			});
			queryClient.setQueryData(
				["adminServiceMonitor", adminToken, monitor.monitor_id],
				monitor,
			);
			pushToast({
				variant: "success",
				message: editing ? "Monitor updated." : "Monitor created.",
			});
			navigate({
				to: "/monitors/$monitorId",
				params: { monitorId: monitor.monitor_id },
			});
		},
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});

	if (!adminToken)
		return <PageState variant="empty" title="Admin token required" />;
	if (capability.unavailable)
		return (
			<CapabilityUnavailableState
				title="Service monitoring unavailable"
				reason={capability.reason}
			/>
		);
	if (editing && monitorQuery.isLoading)
		return <PageState variant="loading" title="Loading monitor" />;
	if (editing && monitorQuery.isError)
		return (
			<PageState
				variant="error"
				title="Failed to load monitor"
				error={monitorQuery.error}
			/>
		);

	const clearDraftEvidence = () => {
		setDraft(null);
		setDraftInterrupted(false);
		setDraftRunId("");
		const search = new URLSearchParams(window.location.search);
		search.delete("draft_run");
		search.delete("draft_coordinator");
		const query = search.toString();
		window.history.replaceState(
			{},
			"",
			`${window.location.pathname}${query ? `?${query}` : ""}`,
		);
	};
	const update = <K extends keyof FormState>(key: K, value: FormState[K]) => {
		setForm((current) => ({ ...current, [key]: value }));
		clearDraftEvidence();
	};
	const httpTarget = form.kind === "http" || form.kind === "https";
	const nodes: AdminNode[] = nodesQuery.data?.items ?? [];
	const selectedNodes = new Set(form.observerPolicy.node_ids);
	const canCreate =
		form.name.trim().length > 0 &&
		(form.observerPolicy.mode === "exclude" ||
			form.observerPolicy.node_ids.length > 0) &&
		(policyCapability.available ||
			form.observerPolicy.mode !== "exclude" ||
			form.observerPolicy.node_ids.length === 0);
	const draftObservers = draft?.observers ?? [];
	const reachedCount = draftObservers.filter(
		(observer) => observer.state === "succeeded",
	).length;
	const draftState = draftInterrupted ? "interrupted" : draft?.state;
	const policySummary =
		form.observerPolicy.mode === "exclude"
			? form.observerPolicy.node_ids.length === 0
				? "All current observer nodes"
				: `All except ${form.observerPolicy.node_ids.length} excluded`
			: `${form.observerPolicy.node_ids.length} included node${
					form.observerPolicy.node_ids.length === 1 ? "" : "s"
				}`;

	return (
		<div className="space-y-6">
			<PageHeader
				title={editing ? "Edit service monitor" : "New service monitor"}
				description={
					"Configure a public target, run optional cluster evidence, then save " +
					"the monitor revision."
				}
			/>
			<div className="@container min-w-0">
				<form
					className="grid gap-8 @min-[68rem]:grid-cols-[minmax(26rem,0.82fr)_minmax(0,1.6fr)]"
					onSubmit={(event) => {
						event.preventDefault();
						if (canCreate) saveMutation.mutate();
					}}
				>
					<section className="min-w-0 space-y-7">
						<section aria-labelledby="monitor-configuration-heading">
							<h2 id="monitor-configuration-heading" className="font-semibold">
								Monitor configuration
							</h2>
							<div className="mt-4 grid min-w-0 gap-4 sm:grid-cols-2">
								<div className="min-w-0 space-y-2 sm:col-span-2">
									<Label htmlFor="monitor-name">Name</Label>
									<Input
										id="monitor-name"
										className="w-full min-w-0"
										value={form.name}
										maxLength={120}
										required
										onChange={(event) => update("name", event.target.value)}
									/>
								</div>
								<div className="space-y-2">
									<Label htmlFor="monitor-kind">Method</Label>
									<Select
										value={form.kind}
										onValueChange={(value) =>
											update("kind", value as ServiceMonitorKind)
										}
									>
										<SelectTrigger id="monitor-kind">
											<SelectValue />
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="https">HTTPS</SelectItem>
											<SelectItem value="http">HTTP</SelectItem>
											<SelectItem value="ping">PING</SelectItem>
											<SelectItem value="tcping">TCPING</SelectItem>
										</SelectContent>
									</Select>
								</div>
								<div className="space-y-2">
									<Label htmlFor="monitor-interval">Interval</Label>
									<Select
										value={form.intervalSeconds}
										onValueChange={(value) =>
											update(
												"intervalSeconds",
												value as FormState["intervalSeconds"],
											)
										}
									>
										<SelectTrigger id="monitor-interval">
											<SelectValue />
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="60">Every 1 minute</SelectItem>
											<SelectItem value="300">Every 5 minutes</SelectItem>
											<SelectItem value="900">Every 15 minutes</SelectItem>
											<SelectItem value="3600">Every hour</SelectItem>
										</SelectContent>
									</Select>
								</div>
								{httpTarget ? (
									<>
										<div className="min-w-0 space-y-2 sm:col-span-2">
											<Label htmlFor="monitor-url">Public URL</Label>
											<Input
												id="monitor-url"
												type="url"
												className="w-full min-w-0"
												value={form.url}
												required
												onChange={(event) => update("url", event.target.value)}
											/>
										</div>
										<div className="space-y-2">
											<Label htmlFor="monitor-http-method">HTTP method</Label>
											<Select
												value={form.method}
												onValueChange={(value) =>
													update("method", value as "get" | "head")
												}
											>
												<SelectTrigger id="monitor-http-method">
													<SelectValue />
												</SelectTrigger>
												<SelectContent>
													<SelectItem value="get">GET</SelectItem>
													<SelectItem value="head">HEAD</SelectItem>
												</SelectContent>
											</Select>
										</div>
										{form.method === "get" ? (
											<div className="space-y-2">
												<Label htmlFor="monitor-body">
													Body contains{" "}
													<span className="font-normal text-muted-foreground">
														(optional)
													</span>
												</Label>
												<Input
													id="monitor-body"
													value={form.bodyContains}
													maxLength={256}
													onChange={(event) =>
														update("bodyContains", event.target.value)
													}
												/>
											</div>
										) : null}
									</>
								) : (
									<>
										<div className="min-w-0 sm:col-span-2 sm:grid sm:grid-cols-[minmax(0,1fr)_12rem]">
											<div className="min-w-0 space-y-2">
												<Label htmlFor="monitor-host">Public host</Label>
												<Input
													id="monitor-host"
													className="w-full min-w-0"
													value={form.host}
													required
													onChange={(event) =>
														update("host", event.target.value)
													}
												/>
											</div>
											{form.kind === "tcping" ? (
												<div className="mt-4 space-y-2 sm:mt-0">
													<Label htmlFor="monitor-port">TCP port</Label>
													<Input
														id="monitor-port"
														type="number"
														min="1"
														max="65535"
														value={form.port}
														required
														onChange={(event) =>
															update("port", event.target.value)
														}
													/>
												</div>
											) : null}
										</div>
									</>
								)}
							</div>
						</section>
						<section
							aria-labelledby="monitor-observer-policy-heading"
							className="border-t border-border/70 pt-6"
						>
							<h2
								id="monitor-observer-policy-heading"
								className="font-semibold"
							>
								Observer policy
							</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Exclude mode is the default. An empty exclusion list means every
								current observer node.
							</p>
							{!policyCapability.available &&
							form.observerPolicy.mode === "exclude" &&
							form.observerPolicy.node_ids.length > 0 ? (
								<p
									className={[
										"mt-2 rounded-lg border border-warning/40 bg-warning/10",
										"px-3 py-2 text-sm text-warning-foreground",
									].join(" ")}
								>
									This server needs an upgrade before exclusions can be saved.
									Use Include only or leave exclusions empty.
								</p>
							) : null}
							<Tabs
								value={form.observerPolicy.mode}
								onValueChange={(value) =>
									update("observerPolicy", {
										mode: value as ObserverPolicy["mode"],
										node_ids: [],
									})
								}
								className="mt-4"
							>
								<TabsList className="w-full justify-start">
									<TabsTrigger value="exclude">Exclude nodes</TabsTrigger>
									<TabsTrigger value="include">Include only</TabsTrigger>
								</TabsList>
							</Tabs>
							<div
								className={[
									"mt-3 flex items-center justify-between gap-3",
									"text-xs text-muted-foreground",
								].join(" ")}
							>
								<span>{policySummary}</span>
								<span>{nodes.length} registered</span>
							</div>
							<fieldset
								className={[
									"mt-3 max-h-56 overflow-y-auto rounded-xl border",
									"border-border/70 p-2",
								].join(" ")}
							>
								<legend className="sr-only">Observer nodes</legend>
								{nodes.length === 0 ? (
									<p className="p-3 text-sm text-muted-foreground">
										Observer nodes will appear here when registered.
									</p>
								) : (
									nodes.map((node) => (
										<div
											key={node.node_id}
											className="flex items-center gap-3 rounded-lg px-3 py-2.5 hover:bg-muted/30"
										>
											<Checkbox
												id={`observer-${node.node_id}`}
												checked={selectedNodes.has(node.node_id)}
												onCheckedChange={(checked) => {
													const next = new Set(selectedNodes);
													if (checked) next.add(node.node_id);
													else next.delete(node.node_id);
													update("observerPolicy", {
														...form.observerPolicy,
														node_ids: [...next],
													});
												}}
											/>
											<label
												htmlFor={`observer-${node.node_id}`}
												className="min-w-0 cursor-pointer"
											>
												<span className="block truncate text-sm">
													{node.node_name}
												</span>
												<span className="block truncate font-mono text-xs text-muted-foreground">
													{node.node_id}
												</span>
											</label>
										</div>
									))
								)}
							</fieldset>
						</section>
						<div className="flex flex-wrap items-center gap-2 border-t border-border/70 pt-6">
							<Button asChild variant="secondary">
								<Link
									to={editing ? "/monitors/$monitorId" : "/monitors"}
									params={editing ? { monitorId: monitorId ?? "" } : undefined}
								>
									Cancel
								</Link>
							</Button>
							<Button
								type="submit"
								loading={saveMutation.isPending}
								disabled={!canCreate}
							>
								{editing ? "Save changes" : "Create monitor"}
							</Button>
						</div>
					</section>
					<section
						aria-labelledby="monitor-cluster-test-heading"
						className={
							"min-w-0 border-t border-border/70 pt-6 " +
							"min-[68rem]:border-l min-[68rem]:border-t-0 " +
							"min-[68rem]:pl-8 min-[68rem]:pt-0"
						}
					>
						<div className="flex flex-wrap items-start justify-between gap-4">
							<div>
								<h2 id="monitor-cluster-test-heading" className="font-semibold">
									Cluster test results
								</h2>
								<p className="mt-1 max-w-xl text-sm text-muted-foreground">
									Optional evidence from a staggered test across the frozen
									observer set. It never blocks creation.
								</p>
							</div>
							<Button
								type="button"
								loading={testMutation.isPending}
								disabled={!draftCapability.available}
								iconLeft={<Icon name="tabler:player-play" />}
								onClick={() => testMutation.mutate()}
							>
								Run cluster test
							</Button>
						</div>
						{!draftCapability.available ? (
							<p
								className={[
									"mt-4 rounded-lg border border-warning/40 bg-warning/10",
									"px-3 py-2 text-sm text-warning-foreground",
								].join(" ")}
							>
								This server does not expose Draft Cluster Test yet. You can
								still create the monitor.
							</p>
						) : null}
						<div
							className="mt-5 flex flex-wrap items-center justify-between gap-3"
							aria-live="polite"
						>
							<div
								className={[
									"w-fit max-w-full rounded-lg border px-3 py-2",
									draftState === "succeeded"
										? "border-success/40 bg-success/10"
										: draftState === "failed" ||
												draftState === "unsupported" ||
												draftState === "interrupted"
											? "border-warning/40 bg-warning/10"
											: "border-border/70 bg-muted/20",
								].join(" ")}
							>
								{draft ? (
									<div className="flex items-center gap-2 font-medium">
										<Icon
											name={
												draftState === "succeeded"
													? "tabler:circle-check"
													: draftState === "failed" ||
															draftState === "interrupted"
														? "tabler:alert-triangle"
														: "tabler:loader-2"
											}
										/>
										{draftState === "running" || draftState === "queued"
											? `${reachedCount} / ${draft.observers.length} observers tested`
											: draftState === "succeeded"
												? `${reachedCount} / ${draft.observers.length} observers reached the target`
												: draftState === "interrupted"
													? "Test interrupted; run it again to collect fresh evidence"
													: `Test ${draftState}`}
									</div>
								) : (
									<p className="text-sm text-muted-foreground">
										Run a test to collect cluster evidence.
									</p>
								)}
							</div>
							<p className="font-mono text-xs text-muted-foreground">
								{draft
									? `coordinator: ${draft.coordinator_node_id}`
									: `observer set: ${policySummary}`}
							</p>
						</div>
						<div className="mt-3 max-h-[32rem] overflow-auto rounded-xl border border-border/70">
							<table className="w-full min-w-[38rem] table-fixed border-collapse text-sm">
								<colgroup>
									<col />
									<col className="w-28" />
									<col className="w-32" />
								</colgroup>
								<thead
									className={[
										"sticky top-0 z-10 bg-background text-xs",
										"font-medium text-muted-foreground",
									].join(" ")}
								>
									<tr className="border-b border-border/70">
										<th className="px-3 py-2.5 text-left font-medium">
											Observer
										</th>
										<th className="px-3 py-2.5 text-right font-medium">
											Response
										</th>
										<th className="px-3 py-2.5 text-right font-medium">
											Result
										</th>
									</tr>
								</thead>
								<tbody className="divide-y divide-border/60">
									{draftObservers.length > 0 ? (
										draftObservers.map((observer) => (
											<tr key={observer.node_id}>
												<td className="px-3 py-3">
													<div className="flex min-w-0 items-center gap-2">
														<Icon
															name={
																observer.state === "succeeded"
																	? "tabler:circle-check"
																	: observer.state === "failed"
																		? "tabler:alert-triangle"
																		: observer.state === "running"
																			? "tabler:loader-2"
																			: "tabler:minus"
															}
															className={
																observer.state === "succeeded"
																	? "text-success"
																	: observer.state === "failed"
																		? "text-warning"
																		: "text-muted-foreground"
															}
														/>
														<span className="truncate font-mono text-xs">
															{observer.node_id}
														</span>
													</div>
												</td>
												<td className="px-3 py-3 text-right tabular-nums text-muted-foreground">
													{observer.latency_ms != null
														? `${observer.latency_ms} ms`
														: "-"}
												</td>
												<td className="px-3 py-3 text-right font-medium">
													{observer.status_code != null
														? `HTTP ${observer.status_code}`
														: (observer.error ??
															observerStatusLabel(observer.state))}
												</td>
											</tr>
										))
									) : (
										<tr>
											<td
												colSpan={3}
												className="px-3 py-8 text-center text-sm text-muted-foreground"
											>
												The result table will populate after the cluster test
												starts.
											</td>
										</tr>
									)}
								</tbody>
							</table>
						</div>
					</section>
				</form>
			</div>
		</div>
	);
}

export function ServiceMonitorNewPage() {
	return <ServiceMonitorEditor />;
}
export function ServiceMonitorEditPage() {
	const { monitorId } = useParams({ from: "/app/monitors/$monitorId/edit" });
	return <ServiceMonitorEditor monitorId={monitorId} />;
}
