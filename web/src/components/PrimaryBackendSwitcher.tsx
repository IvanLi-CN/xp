import { useMemo, useState } from "react";

import {
	type BackendCandidate,
	canonicalBackendOrigin,
	switchPrimaryBackend,
	verifyBackendCandidate,
} from "@/backend/primaryBackend";

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
			<DropdownMenuContent align="end" className="w-80 p-3">
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
							const isChecking = checkingOrigin === candidate.origin;
							return (
								<DropdownMenuItem
									key={candidate.origin}
									disabled={Boolean(checkingOrigin) || isCurrent}
									onSelect={(event) => {
										event.preventDefault();
										void handleSelect(candidate);
									}}
									className="items-start py-2"
								>
									<Icon
										name={isCurrent ? "tabler:circle-check" : "tabler:server"}
										ariaLabel={isCurrent ? "Current" : "Backend"}
										className="mt-0.5 shrink-0"
									/>
									<span className="min-w-0 flex-1">
										<span className="block truncate font-medium">
											{candidate.nodeName}
										</span>
										<span className="block truncate text-xs text-muted-foreground">
											{hostLabel(candidate.origin)}
										</span>
									</span>
									{isChecking ? (
										<span className="text-xs text-muted-foreground">
											Checking
										</span>
									) : null}
								</DropdownMenuItem>
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
