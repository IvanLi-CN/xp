import { useMemo, useState } from "react";

import {
	type BackendCandidate,
	canonicalBackendOrigin,
	currentWebReleaseVersion,
	recognizedReleaseVersion,
	switchPrimaryBackend,
	verifyBackendCandidate,
} from "@/backend/primaryBackend";
import { buildNodePageHref } from "@/utils/navigation";

import { usePrimaryBackend } from "@/backend/PrimaryBackendProvider";
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

function hostLabel(origin: string) {
	try {
		return new URL(origin).host;
	} catch {
		return origin;
	}
}

export function PrimaryBackendSwitcher(props: {
	adminToken: string;
	clusterId: string | null;
	onOpened?: () => void;
	onSwitched?: () => void;
	frontendVersion?: string | null;
}) {
	const backend = usePrimaryBackend();
	const [open, setOpen] = useState(false);
	const [checkingOrigin, setCheckingOrigin] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const candidates = useMemo(
		() =>
			[...backend.candidates].sort((left, right) => {
				if (left.origin === backend.primaryOrigin) return -1;
				if (right.origin === backend.primaryOrigin) return 1;
				return hostLabel(left.origin).localeCompare(hostLabel(right.origin));
			}),
		[backend.candidates, backend.primaryOrigin],
	);

	const handleSelect = async (candidate: BackendCandidate) => {
		if (!props.clusterId || !props.adminToken) {
			setError("Sign in to verify a backend.");
			return;
		}
		if (canonicalBackendOrigin(candidate.origin) === backend.primaryOrigin) {
			setOpen(false);
			return;
		}
		if (candidate.switchable === false) {
			setError(
				candidate.switchBlockedReason ??
					"This node cannot be selected as the primary backend.",
			);
			return;
		}
		setCheckingOrigin(candidate.origin);
		setError(null);
		try {
			const verified = await verifyBackendCandidate({
				origin: candidate.origin,
				clusterId: props.clusterId,
				adminToken: props.adminToken,
			});
			await switchPrimaryBackend(verified);
			props.onSwitched?.();
			setOpen(false);
		} catch (reason) {
			setError(
				reason instanceof Error
					? reason.message
					: "Backend verification failed.",
			);
		} finally {
			setCheckingOrigin(null);
		}
	};

	const frontendVersion =
		props.frontendVersion === undefined
			? currentWebReleaseVersion()
			: recognizedReleaseVersion(props.frontendVersion);
	const versionLabel = (candidate: BackendCandidate) => {
		if (
			!frontendVersion ||
			candidate.versionState !== "ready" ||
			!candidate.serviceVersion
		) {
			return null;
		}
		if (candidate.serviceVersion === frontendVersion) return null;
		return `v${candidate.serviceVersion}`;
	};

	const label =
		backend.state === "unreachable"
			? "Primary backend unavailable"
			: `Primary backend: ${hostLabel(backend.primaryOrigin)}`;

	return (
		<DropdownMenu
			open={open}
			onOpenChange={(nextOpen) => {
				setOpen(nextOpen);
				if (nextOpen) props.onOpened?.();
			}}
		>
			<DropdownMenuTrigger asChild>
				<Button
					variant="secondary"
					size="sm"
					aria-label="Open primary backend"
					title={label}
					disabled={backend.state === "switching"}
					loading={backend.state === "switching"}
				>
					<Icon name="tabler:route" ariaLabel="Primary backend" />
					<span className="hidden max-w-32 truncate sm:inline">
						{hostLabel(backend.primaryOrigin)}
					</span>
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="xp-menu-content">
				<DropdownMenuLabel className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
					Primary backend
				</DropdownMenuLabel>
				<div className="mt-2 space-y-1">
					{candidates.length === 0 ? (
						<p className="px-2 py-2 text-sm text-muted-foreground">
							No registered backends found.
						</p>
					) : (
						candidates.map((candidate) => {
							const isCurrent = candidate.origin === backend.primaryOrigin;
							const resolved = buildNodePageHref(candidate.origin);
							const version = versionLabel(candidate);
							return (
								<div
									key={candidate.origin}
									className="grid grid-cols-[minmax(0,1fr)_auto] gap-1"
								>
									<DropdownMenuItem
										disabled={
											!isCurrent &&
											(Boolean(checkingOrigin) ||
												candidate.switchable === false)
										}
										aria-current={isCurrent ? "true" : undefined}
										onSelect={(event) => {
											event.preventDefault();
											void handleSelect(candidate);
										}}
										className={
											isCurrent
												? "items-start bg-primary/10 py-2 ring-1 ring-inset ring-primary/20 hover:bg-primary/15"
												: "items-start py-2"
										}
										aria-label={
											isCurrent
												? `${candidate.nodeName}, current backend`
												: candidate.switchable === false &&
														candidate.switchBlockedReason
													? `${candidate.nodeName}, ${candidate.switchBlockedReason}`
													: candidate.nodeName
										}
									>
										<Icon
											name={isCurrent ? "tabler:circle-check" : "tabler:server"}
											ariaLabel={isCurrent ? "Current" : "Backend"}
											className={`mt-0.5 shrink-0${isCurrent ? " text-primary" : ""}`}
										/>
										<span className="min-w-0 flex-1">
											<span className="block truncate font-medium">
												{candidate.nodeName}
											</span>
											<span className="block truncate text-xs text-muted-foreground">
												{hostLabel(candidate.origin)}
												{version ? (
													<span className="font-medium text-warning">
														{` · ${version}`}
													</span>
												) : null}
											</span>
										</span>
									</DropdownMenuItem>
									{resolved.disabled ? (
										<DropdownMenuItem
											disabled
											title={resolved.reason}
											aria-label={resolved.reason}
											className="px-2"
										>
											<Icon
												name="tabler:external-link-off"
												ariaLabel="Open node unavailable"
											/>
										</DropdownMenuItem>
									) : (
										<DropdownMenuItem asChild className="px-2">
											<a
												href={resolved.href}
												target="_blank"
												rel="noopener noreferrer"
												title={`Open ${candidate.nodeName} frontend in a new tab`}
												aria-label={`Open ${candidate.nodeName} frontend in a new tab`}
											>
												<Icon
													name="tabler:external-link"
													ariaLabel="Open node frontend"
												/>
											</a>
										</DropdownMenuItem>
									)}
								</div>
							);
						})
					)}
				</div>
				{backend.pendingMutations > 0 ? (
					<p className="mt-2 rounded-md bg-muted px-2 py-1.5 text-xs text-muted-foreground">
						Waiting for a pending change to finish.
					</p>
				) : null}
				{backend.lastSwitchTimedOut ? (
					<p className="mt-2 rounded-md bg-warning/15 px-2 py-1.5 text-xs text-warning-foreground">
						The previous change result is unknown.
					</p>
				) : null}
				{error ? (
					<>
						<DropdownMenuSeparator />
						<p role="alert" className="px-1 text-sm text-destructive">
							{error}
						</p>
					</>
				) : null}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
