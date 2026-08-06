import { FrameworkErrorRecovery } from "../components/FrameworkErrorRecovery";
import type {
	CacheRecoveryResult,
	FrameworkErrorCategory,
} from "../runtime/frameworkErrorRecovery";

const RECOVERY_ERRORS: Record<FrameworkErrorCategory, unknown> = {
	"chunk-load": new Error("Failed to fetch dynamically imported module"),
	"cache-mismatch": new Error("asset cache version mismatch"),
	offline: new TypeError("Failed to fetch"),
	"api-incompatibility": new Error(
		"API compatibility window does not include this client",
	),
	"react-runtime": new Error("Minified React error #185"),
	unknown: new Error("unexpected runtime failure"),
};

function recoveryScenario(): {
	category: FrameworkErrorCategory;
	repeatFailure: boolean;
} {
	const search = new URLSearchParams(window.location.search);
	const requestedCategory = search.get("category") as FrameworkErrorCategory;

	return {
		category:
			requestedCategory in RECOVERY_ERRORS ? requestedCategory : "chunk-load",
		repeatFailure: search.get("repeat") === "1",
	};
}

export function DemoFrameworkRecoveryPage() {
	const { category, repeatFailure } = recoveryScenario();
	const clearCachedApp = async (): Promise<CacheRecoveryResult> => ({
		status: "skipped",
		reason: "replacement-unavailable",
		deleted: [],
	});

	return (
		<FrameworkErrorRecovery
			error={RECOVERY_ERRORS[category]}
			category={category}
			isOnline={category === "offline" ? false : undefined}
			repeatFailure={repeatFailure}
			buildId="3.22.5-demo"
			path="/ui-demo/framework-recovery"
			onReload={() => undefined}
			onClearCachedApp={clearCachedApp}
		/>
	);
}
