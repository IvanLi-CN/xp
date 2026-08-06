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

/*
 * THESIS: A recovery page is one calm decision, not an error dashboard.
 * OWN-WORLD: Existing xp dark control surface, system type, cyan action, semantic state color.
 * STORY: Name the interruption, explain the safe next step, then expose deeper recovery on demand.
 * FIRST VIEWPORT: One narrow column; Reload dominates, cache recovery and diagnostics recede.
 * FORM: Established-world local extension; seed key xp-framework-recovery-operate-v1.
 * FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review,
 * the verdict, and DESIGN.md.
 */
const DESIGN_CONTRACT = "xp-framework-recovery-operate-v1";

const CATEGORY_COPY: Record<
	FrameworkErrorCategory,
	{
		title: string;
		description: string;
		guidance: string;
	}
> = {
	"chunk-load": {
		title: "The page bundle could not be loaded",
		description:
			"A frontend asset is missing or belongs to a different web build.",
		guidance:
			"Reload app first. If this keeps happening, clear the static app cache " +
			"after an online replacement is ready.",
	},
	"cache-mismatch": {
		title: "The cached app assets are out of sync",
		description:
			"The page found static files from different frontend builds and stopped before mixing them.",
		guidance:
			"Reload app first. A safe cache recovery is available once the complete " +
			"replacement build is reachable.",
	},
	offline: {
		title: "You are offline",
		description:
			"The app cannot reach the resources needed to recover this page right now.",
		guidance:
			"Reconnect to the network, then reload app. Existing sign-in and cached " +
			"data are left untouched.",
	},
	"api-incompatibility": {
		title: "The backend does not support this web app",
		description:
			"This web build needs an API capability that the connected server does not provide.",
		guidance:
			"Reload app after the server upgrade completes, or use a web build that " +
			"matches the server release window.",
	},
	"react-runtime": {
		title: "The app hit a runtime error",
		description:
			"The page stopped to avoid leaving the management console in a partial state.",
		guidance:
			"Reload app to start a fresh page. Your sign-in, preferences, and offline " +
			"query cache are not cleared.",
	},
	unknown: {
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
			className="flex min-h-dvh items-center bg-background px-5 py-6 sm:px-8 sm:py-10"
			aria-labelledby="framework-error-title"
			data-design-contract={DESIGN_CONTRACT}
		>
			<div className="mx-auto w-full max-w-lg">
				<div className="mb-10 flex items-center gap-2.5 text-muted-foreground">
					<img
						src="/xp-mark.png"
						alt=""
						aria-hidden="true"
						className="size-8 shrink-0"
					/>
					<div className="text-sm">
						<span className="font-semibold text-foreground">xp</span>
						<span className="mx-2 text-border">/</span>
						<span>cluster manager</span>
					</div>
				</div>

				<section
					data-testid="framework-error-category"
					data-error-category={category}
				>
					<div className="space-y-3">
						<h1
							id="framework-error-title"
							className="text-2xl font-semibold leading-tight sm:text-3xl"
						>
							{copy.title}
						</h1>
						<p className="text-sm leading-6 text-muted-foreground sm:text-base">
							{copy.description}
						</p>
						<p className="text-sm leading-6 text-foreground">{copy.guidance}</p>
					</div>

					{repeatFailure ? (
						<output
							className="mt-5 flex items-start gap-2 text-sm text-warning"
							data-testid="repeat-failure"
						>
							<Icon name="tabler:history" size={18} />
							<p className="leading-5">
								This happened again. Safe cache recovery is available.
							</p>
						</output>
					) : null}

					<div className="mt-7 flex flex-col items-start gap-3">
						<Button
							className="w-full justify-center sm:w-auto"
							onClick={onReload}
							iconLeft={<Icon name="tabler:refresh" size={17} />}
						>
							Reload app
						</Button>
						{showCacheRecovery ? (
							<Button
								variant="ghost"
								className="h-auto min-h-11 px-0 py-1 text-muted-foreground shadow-none"
								onClick={() => void handleCacheRecovery()}
								loading={recoveryState.kind === "working"}
								iconLeft={<Icon name="tabler:database-cog" size={17} />}
							>
								Clear cached app and reload
							</Button>
						) : null}
					</div>

					{recoveryState.kind === "message" ? (
						<output className="mt-4 block text-sm leading-5 text-muted-foreground">
							{recoveryState.message}
						</output>
					) : null}

					<details className="group mt-7 border-t border-border/70 pt-4">
						<summary
							className={[
								"flex min-h-11 cursor-pointer items-center rounded-sm text-sm",
								"font-medium text-muted-foreground focus-visible:ring-2",
								"outline-none transition-colors hover:text-foreground",
								"focus-visible:text-foreground focus-visible:ring-ring/30",
							].join(" ")}
						>
							<Icon
								name="tabler:chevron-right"
								size={16}
								className="mr-1 transition-transform group-open:rotate-90"
							/>
							Technical details
						</summary>
						<pre
							className={[
								"mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-words",
								"rounded-xl bg-muted/60 p-3 font-mono text-xs leading-5",
								"text-muted-foreground [color-scheme:light] dark:[color-scheme:dark]",
							].join(" ")}
							data-testid="framework-error-diagnostics"
						>
							{diagnostics}
						</pre>
						<CopyButton
							className="mt-2 h-auto min-h-11 px-0 py-1 text-muted-foreground shadow-none"
							text={diagnostics}
							label="Copy diagnostic details"
							copiedLabel="Diagnostics copied"
							ariaLabel="Copy diagnostic details"
							variant="ghost"
						/>
					</details>
				</section>
			</div>
		</main>
	);
}
