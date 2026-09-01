import type { FormEvent } from "react";
import { useMemo, useRef, useState } from "react";

import type { ServiceMonitorStatus } from "@/api/adminServiceMonitors";
import { Button } from "@/components/Button";
import { Icon } from "@/components/Icon";
import { PageHeader } from "@/components/PageHeader";
import { ServiceMonitorStatusBadge } from "@/components/ServiceMonitorStatusBadge";
import { ServiceMonitorUptimeBar } from "@/components/ServiceMonitorUptimeBar";
import { useToast } from "@/components/Toast";
import { Badge } from "@/components/ui/badge";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

import { useDemo } from "./store";
import type { DemoScenarioId } from "./types";

type MonitorFilter = "all" | "attention" | "up";
type DemoMonitorKind = "http" | "https" | "ping" | "tcping";
type TargetTestResult = {
	kind: "success" | "failure" | "unsupported" | "error";
	message: string;
	detail?: string;
} | null;

type DemoServiceMonitor = {
	id: string;
	name: string;
	kind: DemoMonitorKind;
	target: string;
	status: ServiceMonitorStatus;
	quality: "Complete history" | "Partial history" | "Local cache";
	availabilityPercent: number | null;
	expected: number;
	executed: number;
	latencyMs: number | null;
	lastChecked: string;
	slots: ServiceMonitorStatus[];
};

const SIX_HOUR_SLOT_COUNT = 72;
const DEMO_TEST_OBSERVERS = [
	{ id: "tokyo-1", location: "Tokyo", latency: "42 ms", result: "HTTP 200" },
	{
		id: "singapore-1",
		location: "Singapore",
		latency: "58 ms",
		result: "HTTP 200",
	},
	{
		id: "frankfurt-1",
		location: "Frankfurt",
		latency: "181 ms",
		result: "HTTP 200",
	},
] as const;

function slots(
	overrides: Readonly<Partial<Record<number, ServiceMonitorStatus>>> = {},
): ServiceMonitorStatus[] {
	return Array.from(
		{ length: SIX_HOUR_SLOT_COUNT },
		(_, index) => overrides[index] ?? "up",
	);
}

function unknownSlots(): ServiceMonitorStatus[] {
	return Array.from({ length: SIX_HOUR_SLOT_COUNT }, () => "unknown");
}

const normalMonitors: DemoServiceMonitor[] = [
	{
		id: "demo-public-landing",
		name: "Public landing page",
		kind: "http",
		target: "https://www.example.net/",
		status: "up",
		quality: "Complete history",
		availabilityPercent: 100,
		expected: 72,
		executed: 72,
		latencyMs: 63,
		lastChecked: "18s ago",
		slots: slots(),
	},
	{
		id: "demo-public-api",
		name: "Public API health",
		kind: "https",
		target: "https://status.example.net/health",
		status: "up",
		quality: "Complete history",
		availabilityPercent: 99.72,
		expected: 360,
		executed: 360,
		latencyMs: 42,
		lastChecked: "18s ago",
		slots: slots({ 33: "degraded" }),
	},
	{
		id: "demo-tcp-edge",
		name: "TCP edge port",
		kind: "tcping",
		target: "edge.example.net:443",
		status: "degraded",
		quality: "Partial history",
		availabilityPercent: 97.22,
		expected: 72,
		executed: 70,
		latencyMs: 218,
		lastChecked: "18s ago",
		slots: slots({ 35: "degraded", 36: "down", 37: "degraded" }),
	},
	{
		id: "demo-dns-reachability",
		name: "DNS reachability",
		kind: "ping",
		target: "resolver.example.net",
		status: "capture_suspended",
		quality: "Local cache",
		availabilityPercent: null,
		expected: 24,
		executed: 0,
		latencyMs: null,
		lastChecked: "18s ago",
		slots: Array.from(
			{ length: SIX_HOUR_SLOT_COUNT },
			() => "capture_suspended",
		),
	},
];

function monitorsForScenario(scenarioId: DemoScenarioId): DemoServiceMonitor[] {
	if (scenarioId === "empty") return [];
	if (scenarioId === "incident") {
		return normalMonitors.map((monitor) => {
			if (monitor.id === "demo-public-api") {
				return {
					...monitor,
					status: "down",
					availabilityPercent: 91.67,
					latencyMs: null,
					slots: slots({
						46: "degraded",
						47: "down",
						48: "down",
						49: "down",
						50: "down",
						51: "down",
					}),
				};
			}
			if (monitor.id === "demo-tcp-edge") {
				return {
					...monitor,
					status: "down",
					availabilityPercent: 87.5,
					latencyMs: null,
					slots: slots({
						31: "down",
						32: "down",
						33: "down",
						34: "down",
						35: "down",
						36: "down",
						37: "down",
						38: "down",
						39: "down",
					}),
				};
			}
			return monitor;
		});
	}
	return normalMonitors;
}

function percentage(value: number | null): string {
	return value === null ? "-" : `${value.toFixed(2)}%`;
}

function isAttention(monitor: DemoServiceMonitor): boolean {
	return monitor.status !== "up";
}

function MonitorRosterRow({ monitor }: { monitor: DemoServiceMonitor }) {
	return (
		<article
			className={[
				"grid gap-x-5 gap-y-3 px-4 py-4 sm:px-5",
				"xl:grid-cols-[minmax(12.5rem,0.8fr)_minmax(30rem,2.6fr)_6rem]",
				"xl:items-center",
			].join(" ")}
		>
			<div className="min-w-0">
				<div className="flex min-w-0 flex-wrap items-center gap-2">
					<p className="min-w-0 truncate font-semibold">{monitor.name}</p>
					<ServiceMonitorStatusBadge status={monitor.status} />
				</div>
				<p className="mt-1 truncate text-xs text-muted-foreground">
					{monitor.kind.toUpperCase()} · {monitor.target}
				</p>
			</div>
			<div className="min-w-0">
				<div className="flex items-baseline justify-between gap-3">
					<p className="text-xs font-medium text-muted-foreground">
						6h availability
					</p>
					<p className="shrink-0 text-lg font-semibold tabular-nums">
						{percentage(monitor.availabilityPercent)}
					</p>
				</div>
				<ServiceMonitorUptimeBar
					className="mt-2"
					label={`${monitor.name} recent six hour check history`}
					prominent
					slots={monitor.slots}
				/>
				<p className="mt-1.5 text-xs text-muted-foreground">
					{monitor.executed}/{monitor.expected} checks · {monitor.quality}
				</p>
			</div>
			<div className="flex items-baseline justify-between gap-3 xl:block xl:text-right">
				<p className="text-xs font-medium text-muted-foreground">Response</p>
				<p className="mt-0.5 text-base font-semibold tabular-nums">
					{monitor.latencyMs === null ? "-" : `${monitor.latencyMs} ms`}
				</p>
				<p className="mt-0.5 text-xs text-muted-foreground">
					{monitor.lastChecked}
				</p>
			</div>
		</article>
	);
}

export function DemoServiceMonitorsPage() {
	const { state } = useDemo();
	const { pushToast } = useToast();
	const [filter, setFilter] = useState<MonitorFilter>("all");
	const [refreshing, setRefreshing] = useState(false);
	const [createOpen, setCreateOpen] = useState(false);
	const [draftName, setDraftName] = useState("");
	const [draftKind, setDraftKind] = useState<DemoMonitorKind>("https");
	const [draftTarget, setDraftTarget] = useState("");
	const [draftPort, setDraftPort] = useState("443");
	const [testingTarget, setTestingTarget] = useState(false);
	const [targetTestResult, setTargetTestResult] =
		useState<TargetTestResult>(null);
	const targetTestSequence = useRef(0);
	const [createdMonitors, setCreatedMonitors] = useState<DemoServiceMonitor[]>(
		[],
	);

	const monitors = useMemo(
		() => [...monitorsForScenario(state.scenarioId), ...createdMonitors],
		[state.scenarioId, createdMonitors],
	);

	const attention = monitors.filter(isAttention);
	const healthy = monitors.filter((monitor) => monitor.status === "up");
	const visibleMonitors =
		filter === "attention" ? attention : filter === "up" ? healthy : monitors;

	function invalidateTargetTest() {
		targetTestSequence.current += 1;
		setTestingTarget(false);
		setTargetTestResult(null);
	}

	function refresh() {
		if (refreshing) return;
		setRefreshing(true);
		window.setTimeout(() => {
			setRefreshing(false);
			pushToast({
				variant: "success",
				message: "Demo observations refreshed.",
			});
		}, 350);
	}

	function createMonitor(event: FormEvent<HTMLFormElement>) {
		event.preventDefault();
		const name = draftName.trim();
		const target = draftTarget.trim();
		const port = Number(draftPort);
		let targetUrlIsValid = true;
		if (draftKind === "http" || draftKind === "https") {
			try {
				const parsed = new URL(target);
				targetUrlIsValid =
					parsed.protocol === `${draftKind}:` && Boolean(parsed.hostname);
			} catch {
				targetUrlIsValid = false;
			}
		}
		if (
			!name ||
			!target ||
			!targetUrlIsValid ||
			(draftKind === "tcping" &&
				(!Number.isInteger(port) || port < 1 || port > 65_535))
		) {
			return;
		}
		if (targetTestResult?.kind !== "success") return;
		setCreatedMonitors((items) => [
			...items,
			{
				id: `demo-monitor-${Date.now()}`,
				name,
				kind: draftKind,
				target: draftKind === "tcping" ? `${target}:${port}` : target,
				status: "unknown",
				quality: "Local cache",
				availabilityPercent: null,
				expected: 0,
				executed: 0,
				latencyMs: null,
				lastChecked: "No check yet",
				slots: unknownSlots(),
			},
		]);
		setDraftName("");
		setDraftKind("https");
		setDraftTarget("");
		setDraftPort("443");
		setCreateOpen(false);
		pushToast({
			variant: "success",
			message: `Monitor ${name} added to the demo.`,
		});
	}

	function testTarget() {
		if (testingTarget) return;
		const testSequence = ++targetTestSequence.current;
		setTestingTarget(true);
		setTargetTestResult(null);
		window.setTimeout(() => {
			if (targetTestSequence.current !== testSequence) return;
			const target = draftTarget.trim();
			const port = Number(draftPort);
			const isUrlMethod = draftKind === "http" || draftKind === "https";
			let valid = Boolean(target);
			if (isUrlMethod) {
				try {
					const parsed = new URL(target);
					valid =
						parsed.protocol === `${draftKind}:` && Boolean(parsed.hostname);
				} catch {
					valid = false;
				}
			}
			if (draftKind === "tcping") {
				valid = valid && Number.isInteger(port) && port >= 1 && port <= 65_535;
			}
			setTestingTarget(false);
			setTargetTestResult(
				!valid
					? { kind: "error", message: "Enter a valid target before testing." }
					: draftKind === "ping"
						? {
								kind: "unsupported",
								message: "PING unsupported on this observer",
								detail: "ICMP capability is unavailable; no TCPING fallback.",
							}
						: target.includes("timeout")
							? {
									kind: "failure",
									message: `${draftKind.toUpperCase()} target failed · timeout`,
									detail: "Total timeout exceeded 10 seconds.",
								}
							: {
									kind: "success",
									message: `${draftKind.toUpperCase()} target reachable · ${
										draftKind === "tcping" ? `${port} ` : ""
									}42 ms`,
									detail:
										"3 of 3 observers completed the staggered cluster test.",
								},
			);
		}, 450);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Service monitoring"
				description="Cluster observer checks and recent six-hour service continuity."
				actions={
					<div className="flex flex-wrap gap-2">
						<Button
							className="w-36"
							variant="secondary"
							loading={refreshing}
							iconLeft={<Icon name="tabler:refresh" />}
							onClick={refresh}
						>
							Refresh
						</Button>
						<Button
							className="w-36"
							onClick={() => setCreateOpen(true)}
							iconLeft={<Icon name="tabler:plus" />}
						>
							New monitor
						</Button>
					</div>
				}
			/>
			{attention.length > 0 ? (
				<section
					aria-labelledby="demo-monitor-attention-heading"
					className="rounded-2xl border border-warning/40 bg-warning/8 px-4 py-3 sm:px-5"
				>
					<div className="flex flex-wrap items-center gap-x-3 gap-y-2">
						<div className="flex items-center gap-2">
							<Icon name="tabler:alert-triangle" className="text-warning" />
							<p id="demo-monitor-attention-heading" className="font-semibold">
								Needs attention
							</p>
							<Badge variant="warning" size="sm">
								{attention.length}
							</Badge>
						</div>
						<p className="text-sm text-muted-foreground">
							{attention.map((monitor) => monitor.name).join(" · ")}
						</p>
					</div>
				</section>
			) : null}
			<section className="overflow-hidden rounded-2xl border border-border/70 bg-card shadow-sm">
				<div
					className={[
						"flex flex-wrap items-center justify-between gap-3",
						"border-b border-border/70 px-4 py-3 sm:px-5",
					].join(" ")}
				>
					<div>
						<h2 className="font-semibold">Monitor roster</h2>
						<p className="mt-0.5 text-xs text-muted-foreground">
							Six-hour continuity is grouped into five-minute buckets.
						</p>
					</div>
					<Tabs
						value={filter}
						onValueChange={(value) => setFilter(value as MonitorFilter)}
					>
						<TabsList aria-label="Monitor filters" className="h-9">
							<TabsTrigger value="all" className="h-7 px-2.5 text-xs">
								All {monitors.length}
							</TabsTrigger>
							<TabsTrigger value="attention" className="h-7 px-2.5 text-xs">
								Attention {attention.length}
							</TabsTrigger>
							<TabsTrigger value="up" className="h-7 px-2.5 text-xs">
								Up {healthy.length}
							</TabsTrigger>
						</TabsList>
					</Tabs>
				</div>
				<div
					className={[
						"hidden gap-x-5 border-b border-border/70 px-5 py-2",
						"text-xs font-medium text-muted-foreground xl:grid",
						"xl:grid-cols-[minmax(12.5rem,0.8fr)_minmax(30rem,2.6fr)_6rem]",
					].join(" ")}
				>
					<span>Monitor</span>
					<span>6h availability and five-minute continuity</span>
					<span className="text-right">Response</span>
				</div>
				<div
					aria-label="Service monitor roster"
					className="divide-y divide-border/70"
				>
					{visibleMonitors.map((monitor) => (
						<MonitorRosterRow key={monitor.id} monitor={monitor} />
					))}
					{visibleMonitors.length === 0 ? (
						<p className="px-5 py-10 text-center text-sm text-muted-foreground">
							No monitors match this filter.
						</p>
					) : null}
				</div>
			</section>

			<Dialog
				open={createOpen}
				onOpenChange={(open) => {
					setCreateOpen(open);
					if (!open) invalidateTargetTest();
				}}
			>
				<DialogContent
					className={[
						"top-4 max-h-[calc(100dvh-2rem)] max-w-5xl",
						"translate-y-0 overflow-y-auto sm:top-1/2",
						"sm:translate-y-[-50%]",
					].join(" ")}
				>
					<DialogHeader>
						<DialogTitle>New monitor</DialogTitle>
						<DialogDescription>
							Add an HTTP, HTTPS, PING, or TCPING public target to this demo
							session.
						</DialogDescription>
					</DialogHeader>
					<form
						className="grid items-start gap-6 md:grid-cols-[minmax(16rem,0.72fr)_minmax(0,1.5fr)]"
						onSubmit={createMonitor}
					>
						<div className="flex min-w-0 flex-col">
							<h3 className="font-semibold">Monitor configuration</h3>
							<div className="mt-4 space-y-4">
								<div className="space-y-2">
									<Label htmlFor="demo-monitor-name">Name</Label>
									<Input
										id="demo-monitor-name"
										value={draftName}
										onChange={(event) => setDraftName(event.target.value)}
										placeholder="Public storefront"
										required
									/>
								</div>
								<div className="space-y-2">
									<Label htmlFor="demo-monitor-kind">Method</Label>
									<Select
										value={draftKind}
										onValueChange={(value) => {
											setDraftKind(value as DemoMonitorKind);
											setDraftTarget("");
											invalidateTargetTest();
										}}
									>
										<SelectTrigger id="demo-monitor-kind">
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
									<Label htmlFor="demo-monitor-target">
										{draftKind === "http" || draftKind === "https"
											? "Public URL"
											: "Public host"}
									</Label>
									<Input
										id="demo-monitor-target"
										value={draftTarget}
										onChange={(event) => {
											setDraftTarget(event.target.value);
											invalidateTargetTest();
										}}
										placeholder={
											draftKind === "https"
												? "https://www.example.net/health"
												: draftKind === "http"
													? "http://www.example.net/health"
													: "edge.example.net"
										}
										type={
											draftKind === "http" || draftKind === "https"
												? "url"
												: "text"
										}
										required
									/>
								</div>
								{draftKind === "tcping" ? (
									<div className="space-y-2">
										<Label htmlFor="demo-monitor-port">Port</Label>
										<Input
											id="demo-monitor-port"
											value={draftPort}
											onChange={(event) => {
												setDraftPort(event.target.value);
												invalidateTargetTest();
											}}
											type="number"
											min={1}
											max={65_535}
											required
										/>
									</div>
								) : null}
							</div>
							<DialogFooter className="mt-auto pt-5 md:justify-end">
								<div className="flex flex-col gap-2 sm:flex-row">
									<Button
										variant="secondary"
										type="button"
										onClick={() => setCreateOpen(false)}
									>
										Cancel
									</Button>
									<Button
										type="submit"
										disabled={targetTestResult?.kind !== "success"}
									>
										Add monitor
									</Button>
								</div>
							</DialogFooter>
						</div>
						<section
							aria-labelledby="demo-target-test-heading"
							className="min-w-0 border-t border-border/70 pt-5 md:border-l md:border-t-0 md:pl-6 md:pt-0"
						>
							<div className="flex items-center justify-between gap-3">
								<div>
									<h3 id="demo-target-test-heading" className="font-semibold">
										Cluster test results
									</h3>
									<p className="mt-1 text-sm text-muted-foreground">
										Target evidence is the primary decision surface before
										creation.
									</p>
								</div>
								<Button
									variant="primary"
									type="button"
									loading={testingTarget}
									iconLeft={<Icon name="tabler:player-play" />}
									onClick={testTarget}
								>
									Run cluster test
								</Button>
							</div>
							<div className="mt-4 flex flex-wrap items-center justify-between gap-3">
								{targetTestResult ? (
									<div
										aria-live="polite"
										className={`w-fit max-w-full rounded-lg border px-3 py-2 ${
											targetTestResult.kind === "success"
												? "border-success/40 bg-success/10"
												: targetTestResult.kind === "unsupported"
													? "border-warning/40 bg-warning/10"
													: "border-destructive/40 bg-destructive/10"
										}`}
									>
										<div className="flex items-center gap-2 font-medium">
											<Icon
												name={
													targetTestResult.kind === "success"
														? "tabler:circle-check"
														: "tabler:alert-triangle"
												}
											/>
											{targetTestResult.kind === "success"
												? "3 / 3 observers reached the target"
												: targetTestResult.message}
										</div>
									</div>
								) : (
									<p className="text-sm text-muted-foreground">
										Run a cluster test to collect observer evidence.
									</p>
								)}
								<p className="font-mono text-xs text-muted-foreground">
									observer set: all-capable
								</p>
							</div>
							<div className="mt-3 overflow-hidden rounded-xl border border-border/70">
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
											<th className="px-3 py-2 text-right font-medium">
												Result
											</th>
										</tr>
									</thead>
									<tbody className="divide-y divide-border/60">
										{targetTestResult?.kind === "success" ? (
											DEMO_TEST_OBSERVERS.map((observer) => (
												<tr key={observer.id}>
													<td className="px-3 py-2.5">
														<div className="flex min-w-0 items-center gap-2">
															<Icon
																name="tabler:circle-check"
																className="text-success"
															/>
															<div className="min-w-0">
																<p className="truncate font-mono text-xs">
																	{observer.id}
																</p>
																<p className="text-xs text-muted-foreground">
																	{observer.location}
																</p>
															</div>
														</div>
													</td>
													<td className="px-3 py-2.5 text-right tabular-nums text-muted-foreground">
														{observer.latency}
													</td>
													<td className="px-3 py-2.5 text-right font-medium">
														{observer.result}
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
						</section>
					</form>
				</DialogContent>
			</Dialog>
		</div>
	);
}
