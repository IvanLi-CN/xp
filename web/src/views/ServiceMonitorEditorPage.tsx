import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import {
	type AdminServiceMonitor,
	type ServiceMonitorKind,
	type ServiceMonitorTarget,
	createAdminServiceMonitor,
	fetchAdminServiceMonitor,
	monitorKind,
	patchAdminServiceMonitor,
	testAdminServiceMonitorTarget,
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
import { Textarea } from "../components/ui/textarea";

type FormState = {
	name: string;
	kind: ServiceMonitorKind;
	url: string;
	host: string;
	port: string;
	method: "get" | "head";
	bodyContains: string;
	intervalSeconds: "60" | "300" | "900" | "3600";
	allCapable: boolean;
	observerNodeIds: string;
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
	allCapable: true,
	observerNodeIds: "",
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
		allCapable: monitor.observer_node_ids === null,
		observerNodeIds: monitor.observer_node_ids?.join("\n") ?? "",
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

function ServiceMonitorEditor({ monitorId }: { monitorId?: string }) {
	const editing = Boolean(monitorId);
	const adminToken = readAdminToken();
	const capability = useApiCapability("admin.service-monitors");
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const [form, setForm] = useState<FormState>(INITIAL_FORM);
	const [testResult, setTestResult] = useState<Awaited<
		ReturnType<typeof testAdminServiceMonitorTarget>
	> | null>(null);
	const testMutation = useMutation({
		mutationFn: () =>
			testAdminServiceMonitorTarget(adminToken, targetFromForm(form)),
		onSuccess: setTestResult,
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});
	const monitorQuery = useQuery({
		queryKey: ["adminServiceMonitor", adminToken, monitorId],
		enabled:
			Boolean(monitorId) && adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminServiceMonitor(adminToken, monitorId ?? "", signal),
	});

	useEffect(() => {
		if (monitorQuery.data) setForm(formFromMonitor(monitorQuery.data));
	}, [monitorQuery.data]);

	const saveMutation = useMutation({
		mutationFn: async () => {
			const payload = {
				name: form.name.trim(),
				target: targetFromForm(form),
				interval_seconds: Number(form.intervalSeconds),
				observer_node_ids: form.allCapable
					? null
					: form.observerNodeIds
							.split(/[\n,]/)
							.map((id) => id.trim())
							.filter(Boolean),
			};
			if (!editing) return createAdminServiceMonitor(adminToken, payload);
			if (!monitorQuery.data) throw new Error("Monitor is not loaded.");
			return patchAdminServiceMonitor(
				adminToken,
				monitorQuery.data.monitor_id,
				{
					...payload,
					expected_revision: monitorQuery.data.revision,
				},
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

	const update = <K extends keyof FormState>(key: K, value: FormState[K]) => {
		setForm((current) => ({ ...current, [key]: value }));
		setTestResult(null);
	};
	const httpTarget = form.kind === "http" || form.kind === "https";
	const testObservations = testResult?.observations ?? [];
	const testSucceeded =
		testObservations.length > 0 &&
		testObservations.every((observation) => observation.outcome === "success");
	const successfulTestObserverCount = testObservations.filter(
		(observation) => observation.outcome === "success",
	).length;
	const observerSetLabel = form.allCapable ? "all-capable" : "allow-list";
	return (
		<div className="space-y-6">
			<PageHeader
				title={editing ? "Edit service monitor" : "New service monitor"}
				description="Changes create a new revision at the next UTC schedule slot."
			/>
			<form
				className="grid gap-8 lg:grid-cols-[minmax(18rem,0.72fr)_minmax(0,1.5fr)]"
				onSubmit={(event) => {
					event.preventDefault();
					saveMutation.mutate();
				}}
			>
				<section className="min-w-0 space-y-6">
					<section aria-labelledby="monitor-configuration-heading">
						<h2 id="monitor-configuration-heading" className="font-semibold">
							Monitor configuration
						</h2>
						<div className="mt-4 grid gap-4 md:grid-cols-2">
							<div className="space-y-2 md:col-span-2">
								<Label htmlFor="monitor-name">Name</Label>
								<Input
									id="monitor-name"
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
									<div className="space-y-2 md:col-span-2">
										<Label htmlFor="monitor-url">Public URL</Label>
										<Input
											id="monitor-url"
											type="url"
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
												Body contains (optional)
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
									<div className="space-y-2">
										<Label htmlFor="monitor-host">Public host</Label>
										<Input
											id="monitor-host"
											value={form.host}
											required
											onChange={(event) => update("host", event.target.value)}
										/>
									</div>
									{form.kind === "tcping" ? (
										<div className="space-y-2">
											<Label htmlFor="monitor-port">TCP port</Label>
											<Input
												id="monitor-port"
												type="number"
												min="1"
												max="65535"
												value={form.port}
												required
												onChange={(event) => update("port", event.target.value)}
											/>
										</div>
									) : null}
								</>
							)}
						</div>
					</section>
					<section
						aria-labelledby="monitor-observer-policy-heading"
						className="border-t border-border/70 pt-5"
					>
						<div>
							<h2
								id="monitor-observer-policy-heading"
								className="font-semibold"
							>
								Observer policy
							</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Choose the nodes that issue scheduled checks and cluster tests.
							</p>
						</div>
						<div className="mt-4 space-y-4">
							<div className="flex items-center gap-3 text-sm">
								<Checkbox
									id="all-capable-observers"
									checked={form.allCapable}
									onCheckedChange={(checked) =>
										update("allCapable", checked === true)
									}
								/>
								<Label htmlFor="all-capable-observers">
									Use every capable observer node
								</Label>
							</div>
							{!form.allCapable ? (
								<div className="space-y-2">
									<Label htmlFor="observer-node-ids">Observer node IDs</Label>
									<Textarea
										id="observer-node-ids"
										value={form.observerNodeIds}
										placeholder="One node ID per line"
										onChange={(event) =>
											update("observerNodeIds", event.target.value)
										}
									/>
								</div>
							) : null}
						</div>
					</section>
					<div className="flex flex-wrap items-center gap-2 border-t border-border/70 pt-5">
						<Button asChild variant="secondary">
							<Link
								to={editing ? "/monitors/$monitorId" : "/monitors"}
								params={editing ? { monitorId: monitorId ?? "" } : undefined}
							>
								Cancel
							</Link>
						</Button>
						<Button type="submit" loading={saveMutation.isPending}>
							{editing ? "Save changes" : "Create monitor"}
						</Button>
					</div>
				</section>
				<section
					aria-labelledby="monitor-cluster-test-heading"
					className="min-w-0 border-t border-border/70 pt-5 lg:border-l lg:border-t-0 lg:pl-7 lg:pt-0"
				>
					<div className="flex items-center justify-between gap-3">
						<div>
							<h2 id="monitor-cluster-test-heading" className="font-semibold">
								Cluster test results
							</h2>
							<p className="mt-1 text-sm text-muted-foreground">
								Target evidence is the primary decision surface before creation.
							</p>
						</div>
						<Button
							type="button"
							loading={testMutation.isPending}
							iconLeft={<Icon name="tabler:player-play" />}
							onClick={() => testMutation.mutate()}
						>
							Run cluster test
						</Button>
					</div>
					<div className="mt-4" aria-live="polite">
						<div className="flex flex-wrap items-center justify-between gap-3">
							{testObservations.length > 0 ? (
								<div
									className={[
										"w-fit max-w-full rounded-lg border px-3 py-2",
										testSucceeded
											? "border-success/40 bg-success/10"
											: "border-warning/40 bg-warning/10",
									].join(" ")}
								>
									<div className="flex items-center gap-2 font-medium">
										<Icon
											name={
												testSucceeded
													? "tabler:circle-check"
													: "tabler:alert-triangle"
											}
										/>
										{successfulTestObserverCount} / {testObservations.length}{" "}
										observers reached the target
									</div>
								</div>
							) : (
								<p className="text-sm text-muted-foreground">
									Run a cluster test to collect observer evidence.
								</p>
							)}
							<p className="font-mono text-xs text-muted-foreground">
								observer set: {observerSetLabel}
							</p>
						</div>
						<div className="mt-3 max-h-80 overflow-y-auto rounded-xl border border-border/70">
							<table className="w-full table-fixed border-collapse text-sm">
								<colgroup>
									<col />
									<col className="w-20" />
									<col className="w-24" />
								</colgroup>
								<thead className="bg-muted/20 text-xs font-medium text-muted-foreground">
									<tr className="border-b border-border/70">
										<th className="px-3 py-2 text-left font-medium">
											Observer
										</th>
										<th className="px-3 py-2 text-right font-medium">
											Response
										</th>
										<th className="px-3 py-2 text-right font-medium">Result</th>
									</tr>
								</thead>
								<tbody className="divide-y divide-border/60">
									{testObservations.length > 0 ? (
										testObservations.map((observation) => (
											<tr key={observation.observer_node_id}>
												<td className="px-3 py-2.5">
													<div className="flex min-w-0 items-center gap-2">
														<Icon
															name={
																observation.outcome === "success"
																	? "tabler:circle-check"
																	: "tabler:alert-triangle"
															}
															className={
																observation.outcome === "success"
																	? "text-success"
																	: "text-warning"
															}
														/>
														<span className="truncate font-mono text-xs">
															{observation.observer_node_id}
														</span>
													</div>
												</td>
												<td className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">
													{observation.latency_ms != null
														? `${observation.latency_ms} ms`
														: "-"}
												</td>
												<td className="px-3 py-2.5 text-right font-medium">
													{observation.status_code != null
														? `HTTP ${observation.status_code}`
														: (observation.error ?? observation.outcome)}
												</td>
											</tr>
										))
									) : (
										<tr>
											<td
												colSpan={3}
												className="px-3 py-4 text-sm text-muted-foreground"
											>
												Run a cluster test to collect results from each
												observer.
											</td>
										</tr>
									)}
								</tbody>
							</table>
						</div>
					</div>
				</section>
			</form>
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
