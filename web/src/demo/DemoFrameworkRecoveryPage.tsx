import { useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { FrameworkErrorRecovery } from "../components/FrameworkErrorRecovery";
import { Icon } from "../components/Icon";
import type {
	CacheRecoveryResult,
	FrameworkErrorCategory,
} from "../runtime/frameworkErrorRecovery";

type RecoveryDemoState = {
	category: FrameworkErrorCategory;
	error: unknown;
	isOnline?: boolean;
};

const RECOVERY_STATES: Array<{
	category: FrameworkErrorCategory;
	label: string;
	icon: string;
	error: unknown;
	isOnline?: boolean;
}> = [
	{
		category: "chunk-load",
		label: "Resource load",
		icon: "tabler:cloud-off",
		error: new Error("Failed to fetch dynamically imported module"),
	},
	{
		category: "cache-mismatch",
		label: "Cache mismatch",
		icon: "tabler:database-x",
		error: new Error("asset cache version mismatch"),
	},
	{
		category: "offline",
		label: "Offline",
		icon: "tabler:wifi-off",
		error: new TypeError("Failed to fetch"),
		isOnline: false,
	},
	{
		category: "api-incompatibility",
		label: "API mismatch",
		icon: "tabler:plug-connected-x",
		error: new Error("API compatibility window does not include this client"),
	},
	{
		category: "react-runtime",
		label: "React runtime",
		icon: "tabler:brand-react",
		error: new Error("Minified React error #185"),
	},
	{
		category: "unknown",
		label: "Unknown",
		icon: "tabler:alert-triangle",
		error: new Error("unexpected runtime failure"),
	},
];

const initialState = RECOVERY_STATES[0];

export function DemoFrameworkRecoveryPage() {
	const [selected, setSelected] = useState<RecoveryDemoState>(initialState);
	const [repeatFailure, setRepeatFailure] = useState(false);
	const [lastAction, setLastAction] = useState("No recovery action run");

	const clearCachedApp = async (): Promise<CacheRecoveryResult> => {
		setLastAction(
			"Mock probe blocked cache clearing; existing cache is preserved",
		);
		return {
			status: "skipped",
			reason: "replacement-unavailable",
			deleted: [],
		};
	};

	return (
		<div className="min-h-screen bg-background px-4 py-5 md:px-6 md:py-8">
			<main className="mx-auto max-w-7xl space-y-5">
				<header
					className={[
						"flex flex-col gap-4 rounded-2xl border border-border/70",
						"bg-card px-5 py-4 shadow-sm sm:flex-row sm:items-center",
						"sm:justify-between",
					].join(" ")}
				>
					<div className="flex min-w-0 items-center gap-3">
						<img src="/xp-mark.png" alt="xp" className="size-9 shrink-0" />
						<div className="min-w-0">
							<h1 className="text-lg font-semibold">Framework recovery</h1>
							<p className="truncate text-sm text-muted-foreground">
								Controlled error states for the xp web shell
							</p>
						</div>
					</div>
					<div className="flex flex-wrap items-center gap-2">
						<Badge variant="info">mock-only</Badge>
						<Badge variant="ghost">no backend</Badge>
					</div>
				</header>

				<div className="grid gap-5 lg:grid-cols-[18rem_minmax(0,1fr)] lg:items-start">
					<aside className="xp-panel p-4">
						<div className="flex items-start justify-between gap-3">
							<div>
								<p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">
									Error category
								</p>
								<p className="mt-1 text-sm text-muted-foreground">
									Select a deterministic failure state.
								</p>
							</div>
							<Badge variant="ghost" size="sm">
								6 states
							</Badge>
						</div>

						<div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-1">
							{RECOVERY_STATES.map((state) => (
								<Button
									key={state.category}
									variant={
										selected.category === state.category
											? "primary"
											: "secondary"
									}
									size="sm"
									className="justify-start"
									onClick={() => {
										setSelected(state);
										setLastAction("No recovery action run");
									}}
								>
									<Icon name={state.icon} size={16} />
									{state.label}
								</Button>
							))}
						</div>

						<div className="mt-5 border-t border-border/70 pt-4">
							<p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">
								Failure history
							</p>
							<div className="mt-3 flex flex-wrap items-center gap-2">
								<Badge variant={repeatFailure ? "warning" : "success"}>
									{repeatFailure ? "repeated" : "first failure"}
								</Badge>
								<Button
									variant="ghost"
									size="sm"
									onClick={() => setRepeatFailure((current) => !current)}
								>
									Toggle repeated state
								</Button>
							</div>
						</div>

						<div className="mt-5 rounded-xl border border-border/70 bg-muted/35 p-3">
							<p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">
								Last action
							</p>
							<p className="mt-2 text-sm leading-5 text-foreground">
								{lastAction}
							</p>
						</div>
					</aside>

					<section className="min-w-0" aria-label="Recovery preview">
						<FrameworkErrorRecovery
							error={selected.error}
							category={selected.category}
							isOnline={selected.isOnline}
							repeatFailure={repeatFailure}
							buildId="3.22.5-demo"
							path="/ui-demo/framework-recovery"
							onReload={() => setLastAction("Mock reload requested")}
							onClearCachedApp={clearCachedApp}
						/>
					</section>
				</div>
			</main>
		</div>
	);
}
