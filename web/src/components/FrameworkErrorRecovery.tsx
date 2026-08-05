import { useMemo, useState } from "react";

import {
	type CacheRecoveryResult,
	type FrameworkErrorCategory,
	classifyFrameworkError,
	createDiagnosticDetails,
	recoverXpAppShell,
} from "../runtime/frameworkErrorRecovery";
import { Button } from "./Button";
import { CopyButton } from "./CopyButton";
import { Icon } from "./Icon";

type FrameworkErrorRecoveryProps = {
	error: unknown;
	category?: FrameworkErrorCategory;
	buildId?: string;
	path?: string;
	isOnline?: boolean;
	repeatFailure?: boolean;
	onReload?: () => void;
	onClearCachedApp?: () => Promise<CacheRecoveryResult>;
};

type RecoveryState =
	| { kind: "idle" }
	| { kind: "working" }
	| { kind: "message"; message: string };

const CATEGORY_COPY: Record<
	FrameworkErrorCategory,
	{
		label: string;
		icon: string;
		title: string;
		description: string;
		guidance: string;
	}
> = {
	"chunk-load": {
		label: "resource load failure",
		icon: "tabler:cloud-off",
		title: "The page bundle could not be loaded",
		description:
			"A frontend asset is missing or belongs to a different web build.",
		guidance:
			"Reload app first. If this keeps happening, clear the static app cache " +
			"after an online replacement is ready.",
	},
	"cache-mismatch": {
		label: "cache version mismatch",
		icon: "tabler:database-x",
		title: "The cached app assets are out of sync",
		description:
			"The page found static files from different frontend builds and stopped before mixing them.",
		guidance:
			"Reload app first. A safe cache recovery is available once the complete " +
			"replacement build is reachable.",
	},
	offline: {
		label: "offline",
		icon: "tabler:wifi-off",
		title: "You are offline",
		description:
			"The app cannot reach the resources needed to recover this page right now.",
		guidance:
			"Reconnect to the network, then reload app. Existing sign-in and cached " +
			"data are left untouched.",
	},
	"api-incompatibility": {
		label: "API incompatibility",
		icon: "tabler:plug-connected-x",
		title: "The backend does not support this web app",
		description:
			"This web build needs an API capability that the connected server does not provide.",
		guidance:
			"Reload app after the server upgrade completes, or use a web build that " +
			"matches the server release window.",
	},
	"react-runtime": {
		label: "React runtime failure",
		icon: "tabler:brand-react",
		title: "The app hit a runtime error",
		description:
			"The page stopped to avoid leaving the management console in a partial state.",
		guidance:
			"Reload app to start a fresh page. Your sign-in, preferences, and offline " +
			"query cache are not cleared.",
	},
	unknown: {
		label: "unknown runtime failure",
		icon: "tabler:alert-triangle",
		title: "The app encountered an unexpected error",
		description:
			"xp could not safely continue this page, but the rest of your browser data is preserved.",
		guidance:
			"Reload app to try again. Copy the diagnostic details if the problem returns.",
	},
};

function defaultBuildId(): string {
	try {
		return __XP_WEB_BUILD_ID__;
	} catch {
		return "development";
	}
}

function defaultReload(): void {
	if (typeof window !== "undefined") window.location.reload();
}

function recoveryMessage(
	result: Extract<CacheRecoveryResult, { status: "skipped" }>,
): string {
	if (result.reason === "offline") {
		return "Reconnect before trying cache recovery. The current app cache was left untouched.";
	}
	if (result.reason === "replacement-unavailable") {
		return (
			"The current app cache was left untouched because a complete replacement " +
			"was not available."
		);
	}
	if (result.reason === "service-worker-unavailable") {
		return "This browser has no active app cache to recover. Reload app instead.";
	}
	return "The app cache could not be changed. Reload app and try again later.";
}

export function FrameworkErrorRecovery({
	error,
	category: categoryProp,
	buildId = defaultBuildId(),
	path = typeof window === "undefined" ? "/" : window.location.pathname,
	isOnline,
	repeatFailure = false,
	onReload = defaultReload,
	onClearCachedApp = () => recoverXpAppShell({ buildId, isOnline }),
}: FrameworkErrorRecoveryProps) {
	const category = categoryProp ?? classifyFrameworkError(error, { isOnline });
	const copy = CATEGORY_COPY[category];
	const [recoveryState, setRecoveryState] = useState<RecoveryState>({
		kind: "idle",
	});
	const diagnostics = useMemo(
		() =>
			createDiagnosticDetails({
				error,
				category,
				buildId,
				path,
				isOnline,
				repeatFailure,
			}),
		[buildId, category, error, isOnline, path, repeatFailure],
	);
	const showCacheRecovery =
		repeatFailure || category === "chunk-load" || category === "cache-mismatch";

	async function handleCacheRecovery() {
		setRecoveryState({ kind: "working" });
		try {
			const result = await onClearCachedApp();
			if (result.status === "cleared") {
				setRecoveryState({
					kind: "message",
					message: "Replacement ready. Reloading app...",
				});
				onReload();
				return;
			}
			setRecoveryState({ kind: "message", message: recoveryMessage(result) });
		} catch {
			setRecoveryState({
				kind: "message",
				message:
					"The current app cache was left untouched because recovery could not be verified.",
			});
		}
	}

	return (
		<main
			className="flex min-h-screen items-center justify-center bg-background px-4 py-8 sm:px-6"
			aria-labelledby="framework-error-title"
		>
			<div className="w-full max-w-2xl">
				<div className="mb-5 flex items-center gap-3 px-1">
					<img src="/xp-mark.png" alt="xp" className="size-9 shrink-0" />
					<div>
						<p className="text-sm font-semibold text-foreground">xp</p>
						<p className="text-xs text-muted-foreground">cluster manager</p>
					</div>
				</div>

				<section className="xp-card overflow-hidden">
					<div className="xp-card-body gap-6 p-6 sm:p-8">
						<div className="flex items-start gap-4">
							<div
								className={[
									"flex size-12 shrink-0 items-center justify-center",
									"rounded-2xl bg-destructive/12 text-destructive",
								].join(" ")}
							>
								<Icon name={copy.icon} size={25} ariaLabel="Error" />
							</div>
							<div className="min-w-0 space-y-2">
								<p
									className="text-xs font-semibold uppercase tracking-[0.16em] text-destructive"
									data-testid="framework-error-category"
								>
									{copy.label}
								</p>
								<h1
									id="framework-error-title"
									className="text-2xl font-semibold leading-tight tracking-tight sm:text-3xl"
								>
									{copy.title}
								</h1>
								<p className="max-w-xl text-sm leading-6 text-muted-foreground sm:text-base">
									{copy.description}
								</p>
							</div>
						</div>

						<div className="xp-panel-muted p-4">
							<p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">
								Recommended next step
							</p>
							<p className="mt-2 text-sm leading-6 text-foreground">
								{copy.guidance}
							</p>
						</div>

						{repeatFailure ? (
							<output
								className="xp-alert xp-alert-warning"
								data-testid="repeat-failure"
							>
								<Icon
									name="tabler:history"
									size={18}
									ariaLabel="Repeated failure"
								/>
								<p className="text-sm">
									This failure has repeated in this tab. A guarded static cache
									recovery is now available.
								</p>
							</output>
						) : null}

						<div className="xp-card-actions">
							<Button
								onClick={onReload}
								iconLeft={<Icon name="tabler:refresh" size={17} />}
							>
								Reload app
							</Button>
							{showCacheRecovery ? (
								<Button
									variant="secondary"
									onClick={() => void handleCacheRecovery()}
									loading={recoveryState.kind === "working"}
									iconLeft={<Icon name="tabler:database-cog" size={17} />}
								>
									Clear cached app and reload
								</Button>
							) : null}
							<CopyButton
								text={diagnostics}
								label="Copy diagnostic details"
								copiedLabel="Diagnostics copied"
								ariaLabel="Copy diagnostic details"
								variant="ghost"
							/>
						</div>

						{recoveryState.kind === "message" ? (
							<output className="text-sm text-muted-foreground">
								{recoveryState.message}
							</output>
						) : null}

						<details className="group border-t border-border/70 pt-4">
							<summary
								className={[
									"cursor-pointer text-sm font-medium text-muted-foreground",
									"outline-none transition-colors hover:text-foreground",
									"focus-visible:text-foreground",
								].join(" ")}
							>
								Technical details
							</summary>
							<pre
								className={[
									"mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-words",
									"rounded-xl bg-muted/60 p-3 font-mono text-xs leading-5",
									"text-muted-foreground",
								].join(" ")}
								data-testid="framework-error-diagnostics"
							>
								{diagnostics}
							</pre>
						</details>
					</div>
				</section>
			</div>
		</main>
	);
}
