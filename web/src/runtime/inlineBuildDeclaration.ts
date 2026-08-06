import { inlineBootstrapFallback } from "./inlineBootstrapFallback";

export function serializeInlineScriptString(value: string): string {
	return JSON.stringify(value)
		.replace(/</g, "\\u003c")
		.replace(/>/g, "\\u003e")
		.replace(/&/g, "\\u0026")
		.replace(/\u2028/g, "\\u2028")
		.replace(/\u2029/g, "\\u2029");
}

export function transformIndexHtmlWithInlineBuildDeclaration(
	html: string,
	buildId: string,
): string {
	const declaration = [
		"<script>",
		inlineBootstrapFallback(buildId),
		`window.__XP_WEB_BUILD_ID__=${serializeInlineScriptString(buildId)};`,
		"if (navigator.serviceWorker?.controller) {",
		"navigator.serviceWorker.controller.postMessage({",
		'type: "XP_DECLARE_BUILD",',
		"buildId: window.__XP_WEB_BUILD_ID__",
		"});",
		"}",
		"</script>",
	].join("");
	const entry = `?xp-build=${encodeURIComponent(buildId)}`;
	return html
		.replace(/(src="[^\"]+\.(?:js|tsx))"/, `$1${entry}"`)
		.replace("<head>", `<head>${declaration}`);
}
