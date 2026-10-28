import { useCallback, useEffect, useMemo, useState } from "react";

import { usePrimaryBackend } from "@/backend/PrimaryBackendProvider";
import {
	type BackendCandidate,
	canonicalBackendOrigin,
	isStaticWebConsole,
	probeBackendCandidate,
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
		async (candidate: BackendCandidate) => {
			const origin = candidate.origin;
			if (!backend.clusterId && origin !== pageOrigin) {
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
				if (backend.clusterId) {
					await probeBackendCandidate({
						origin,
						clusterId: backend.clusterId,
					});
				}
				setProbeStatus((current) => ({ ...current, [origin]: "ready" }));
			} catch (error) {
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
		setSelectionError(null);
		setProbeStatus({});
		setProbeErrors({});
		for (const candidate of candidates) void runProbe(candidate);
	}, [candidates, runProbe, staticConsole]);

	const currentTargetReady =
		staticConsole ||
		(!backend.clusterId && backend.primaryOrigin === pageOrigin) ||
		probeStatus[backend.primaryOrigin] === "ready";

	useEffect(() => {
		onProbeStateChange?.(currentTargetReady);
	}, [currentTargetReady, onProbeStateChange]);

	const handleSelect = async (candidate: BackendCandidate) => {
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

	return (
		<div className="space-y-2">
			<p className="text-sm font-medium">Service node</p>
			<DropdownMenu
				open={open}
				onOpenChange={(nextOpen) => {
					setOpen(nextOpen);
					if (nextOpen) setSelectionError(null);
				}}
			>
				<DropdownMenuTrigger asChild>
					<Button
						variant="outline"
						className="w-full justify-between"
						aria-label="Select service node"
						disabled={backend.state === "switching" || candidates.length === 0}
						loading={backend.state === "switching"}
					>
						<span className="flex min-w-0 items-center gap-2">
							<Icon name="tabler:route" ariaLabel="Service node" />
							<span className="truncate">
								{currentCandidate
									? hostLabel(currentCandidate.origin)
									: "No service node available"}
							</span>
						</span>
						<Icon name="tabler:chevron-down" ariaLabel="Open" />
					</Button>
				</DropdownMenuTrigger>
				<DropdownMenuContent
					align="start"
					className="xp-menu-content w-72 max-w-[calc(100vw-2rem)]"
				>
					<DropdownMenuLabel className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
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
									status === undefined || status === "checking";
								const isError = status === "error";
								const isSelecting = selectingOrigin === candidate.origin;
								return (
									<DropdownMenuItem
										key={candidate.origin}
										disabled={
											isCurrent || isChecking || Boolean(selectingOrigin)
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
											{isSelecting
												? "Selecting"
												: isChecking
													? "Checking"
													: isCurrent
														? "Current"
														: isError
															? "Retry"
															: "Ready"}
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
	);
}
