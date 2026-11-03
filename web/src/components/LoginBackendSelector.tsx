import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { usePrimaryBackend } from "@/backend/PrimaryBackendProvider";
import {
	type BackendCandidate,
	canonicalBackendOrigin,
	hydratePrimaryBackendProfile,
	isStaticWebConsole,
	probeBackendCandidate,
	probeBackendOrigin,
	selectPreAuthBackend,
} from "@/backend/primaryBackend";

import { Button } from "./Button";
import { Icon } from "./Icon";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "./ui/dropdown-menu";

type ProbeStatus = "checking" | "ready" | "error";

type LoginBackendSelectorProps = {
	className?: string;
	disabled?: boolean;
	onProbeStateChange?: (ready: boolean) => void;
	staticConsole?: boolean;
};

function hostLabel(origin: string) {
	try {
		return new URL(origin).host;
	} catch {
		return origin;
	}
}

function candidateLabel(candidate: BackendCandidate) {
	const name =
		typeof candidate.nodeName === "string" ? candidate.nodeName.trim() : "";
	return name && name !== "Current page" ? name : hostLabel(candidate.origin);
}

function currentPageCandidate(): BackendCandidate | null {
	if (typeof window === "undefined") return null;
	const origin = canonicalBackendOrigin(window.location.origin);
	return origin
		? {
				origin,
				nodeId: "current-page",
				nodeName: "Current page",
				verifiedAt: Date.now(),
				lastError: null,
			}
		: null;
}

export function LoginBackendSelector({
	className,
	disabled = false,
	onProbeStateChange,
	staticConsole: staticConsoleProp = false,
}: LoginBackendSelectorProps) {
	const staticConsole = staticConsoleProp || isStaticWebConsole();
	const backend = usePrimaryBackend();
	const [open, setOpen] = useState(false);
	const [probeStatus, setProbeStatus] = useState<Record<string, ProbeStatus>>(
		{},
	);
	const [probeErrors, setProbeErrors] = useState<Record<string, string>>({});
	const [selectingOrigin, setSelectingOrigin] = useState<string | null>(null);
	const [selectionError, setSelectionError] = useState<string | null>(null);
	const probeEpoch = useRef(0);
	const candidates = useMemo(() => {
		const byOrigin = new Map<string, BackendCandidate>();
		const current = currentPageCandidate();
		if (current) byOrigin.set(current.origin, current);
		for (const candidate of backend.candidates) {
			const origin = canonicalBackendOrigin(candidate.origin);
			if (!origin) continue;
			byOrigin.set(origin, { ...candidate, origin });
		}
		return [...byOrigin.values()].sort((left, right) => {
			if (left.origin === backend.primaryOrigin) return -1;
			if (right.origin === backend.primaryOrigin) return 1;
			return hostLabel(left.origin).localeCompare(hostLabel(right.origin));
		});
	}, [backend.candidates, backend.primaryOrigin]);
	const pageOrigin =
		typeof window === "undefined" ? "" : window.location.origin;

	const runProbe = useCallback(
		async (
			candidate: BackendCandidate,
			epoch = probeEpoch.current,
			signal?: AbortSignal,
		) => {
			const origin = candidate.origin;
			const isCurrentEpoch = () => epoch === probeEpoch.current;
			if (signal?.aborted || !isCurrentEpoch()) return;
			if (!backend.clusterId && origin !== pageOrigin) {
				if (!isCurrentEpoch()) return;
				setProbeStatus((current) => ({ ...current, [origin]: "error" }));
				setProbeErrors((current) => ({
					...current,
					[origin]: "Cluster identity is not available for this candidate.",
				}));
				return;
			}
			setProbeStatus((current) => ({ ...current, [origin]: "checking" }));
			setProbeErrors((current) => {
				const next = { ...current };
				delete next[origin];
				return next;
			});
			try {
				if (origin === pageOrigin) {
					const result = await probeBackendOrigin({ origin, signal });
					if (signal?.aborted || !isCurrentEpoch()) return;
					if (result.clusterId !== backend.clusterId) {
						probeEpoch.current += 1;
						hydratePrimaryBackendProfile(result.clusterId);
						return;
					}
				} else if (backend.clusterId) {
					await probeBackendCandidate({
						origin,
						clusterId: backend.clusterId,
						signal,
					});
				} else {
					await probeBackendOrigin({ origin, signal });
				}
				if (signal?.aborted || !isCurrentEpoch()) return;
				setProbeStatus((current) => ({ ...current, [origin]: "ready" }));
			} catch (error) {
				if (signal?.aborted || !isCurrentEpoch()) return;
				setProbeStatus((current) => ({ ...current, [origin]: "error" }));
				setProbeErrors((current) => ({
					...current,
					[origin]:
						error instanceof Error ? error.message : "Backend probe failed.",
				}));
			}
		},
		[backend.clusterId, pageOrigin],
	);

	useEffect(() => {
		if (staticConsole) return;
		const epoch = ++probeEpoch.current;
		const controller = new AbortController();
		setSelectionError(null);
		setProbeStatus({});
		setProbeErrors({});
		for (const candidate of candidates)
			void runProbe(candidate, epoch, controller.signal);
		return () => {
			controller.abort();
			if (probeEpoch.current === epoch) probeEpoch.current += 1;
		};
	}, [candidates, runProbe, staticConsole]);
	useEffect(() => {
		if (backend.lastSwitchTimedOut) {
			setSelectionError(
				"The previous write result is unknown. Verify the service node before retrying.",
			);
		}
	}, [backend.lastSwitchTimedOut]);

	const currentTargetReady =
		staticConsole || probeStatus[backend.primaryOrigin] === "ready";

	useEffect(() => {
		onProbeStateChange?.(currentTargetReady);
	}, [currentTargetReady, onProbeStateChange]);

	const handleSelect = async (candidate: BackendCandidate) => {
		if (disabled) return;
		const status = probeStatus[candidate.origin];
		if (status === "error") {
			void runProbe(candidate);
			return;
		}
		if (status !== "ready" || candidate.origin === backend.primaryOrigin) {
			return;
		}
		setSelectingOrigin(candidate.origin);
		setSelectionError(null);
		try {
			const result = await selectPreAuthBackend(candidate.origin);
			if (result.timedOut) {
				setSelectionError(
					"The previous write result is unknown. Verify the service node before retrying.",
				);
			}
			setOpen(false);
		} catch (error) {
			setSelectionError(
				error instanceof Error ? error.message : "Backend selection failed.",
			);
		} finally {
			setSelectingOrigin(null);
		}
	};

	const currentCandidate =
		candidates.find(
			(candidate) => candidate.origin === backend.primaryOrigin,
		) ?? candidates[0];
	if (staticConsole) return null;
	const candidateStatus = (candidate: BackendCandidate) => {
		const isCurrent = candidate.origin === backend.primaryOrigin;
		const status = probeStatus[candidate.origin];
		if (selectingOrigin === candidate.origin) return "Selecting";
		if (!isCurrent && (status === undefined || status === "checking"))
			return "Checking";
		if (isCurrent) return "Current";
		return status === "error" ? "Retry" : "Ready";
	};

	return (
		<div className={className}>
			<div className="space-y-2">
				<p className="text-sm font-medium md:hidden">Service node</p>
				<DropdownMenu
					open={open}
					onOpenChange={(nextOpen) => {
						setOpen(nextOpen);
						if (nextOpen) setSelectionError(null);
					}}
				>
					<div className="grid grid-cols-[minmax(0,1fr)]">
						{candidates.map((candidate) => (
							<span
								key={candidate.origin}
								aria-hidden="true"
								className="invisible col-start-1 row-start-1 hidden items-center gap-2 px-[13px] md:flex"
							>
								<span className="size-[18px] shrink-0" />
								<span className="flex min-w-0 flex-col">
									<span className="truncate text-sm font-medium">
										{candidateLabel(candidate)}
									</span>
									<span className="truncate text-xs">
										{hostLabel(candidate.origin)}
									</span>
								</span>
								<span className="shrink-0 text-xs">
									{candidateStatus(candidate)}
								</span>
							</span>
						))}
						<DropdownMenuTrigger asChild>
							<Button
								variant="outline"
								className="col-start-1 row-start-1 w-full justify-between"
								aria-label="Select service node"
								disabled={
									disabled ||
									backend.state === "switching" ||
									candidates.length === 0
								}
								loading={backend.state === "switching"}
							>
								<span className="flex min-w-0 items-center gap-2">
									<Icon
										name="tabler:route"
										ariaLabel="Service node"
										className="md:hidden"
									/>
									<span className="truncate">
										{currentCandidate
											? candidateLabel(currentCandidate)
											: "No service node available"}
									</span>
								</span>
								<Icon name="tabler:chevron-down" ariaLabel="Open" />
							</Button>
						</DropdownMenuTrigger>
					</div>
					<DropdownMenuContent
						align="end"
						className={[
							"xp-menu-content w-72 max-w-[calc(100vw-2rem)]",
							"md:w-[var(--radix-dropdown-menu-trigger-width)]",
							"md:min-w-0 md:max-w-[min(24rem,calc(100vw-3rem))]",
						].join(" ")}
					>
						<DropdownMenuLabel
							className={[
								"px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground",
								"md:sr-only",
							].join(" ")}
						>
							Available service nodes
						</DropdownMenuLabel>
						<div className="mt-2 space-y-1">
							{candidates.length === 0 ? (
								<p className="px-2 py-2 text-sm text-muted-foreground">
									No service nodes found.
								</p>
							) : (
								candidates.map((candidate) => {
									const status = probeStatus[candidate.origin];
									const isCurrent = candidate.origin === backend.primaryOrigin;
									const isChecking =
										!isCurrent &&
										(status === undefined || status === "checking");
									const isError = status === "error";
									return (
										<DropdownMenuItem
											key={candidate.origin}
											disabled={
												(isCurrent && !isError) ||
												isChecking ||
												Boolean(selectingOrigin) ||
												disabled
											}
											onSelect={(event) => {
												event.preventDefault();
												void handleSelect(candidate);
											}}
											className="items-start py-2"
										>
											<Icon
												name={
													isCurrent
														? "tabler:circle-check"
														: isError
															? "tabler:refresh"
															: "tabler:server"
												}
												ariaLabel={isError ? "Retry" : "Service node"}
												className="mt-0.5 shrink-0"
											/>
											<span className="min-w-0 flex-1">
												<span className="block truncate font-medium">
													{candidateLabel(candidate)}
												</span>
												<span className="block truncate text-xs text-muted-foreground">
													{hostLabel(candidate.origin)}
												</span>
											</span>
											<span className="shrink-0 text-xs text-muted-foreground">
												{candidateStatus(candidate)}
											</span>
										</DropdownMenuItem>
									);
								})
							)}
						</div>
						{Object.values(probeErrors).length > 0 ? (
							<>
								<DropdownMenuSeparator />
								<p role="alert" className="px-1 text-sm text-destructive">
									{Object.values(probeErrors)[0]}
								</p>
							</>
						) : null}
					</DropdownMenuContent>
				</DropdownMenu>
				{selectionError ? (
					<p role="alert" className="text-sm text-destructive">
						{selectionError}
					</p>
				) : null}
			</div>
		</div>
	);
}
