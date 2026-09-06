import { inlineBootstrapFallback } from "./inlineBootstrapFallback";

export function serializeInlineScriptString(value: string): string {
	return JSON.stringify(value)
		.replace(/</g, "\\u003c")
		.replace(/>/g, "\\u003e")
		.replace(/&/g, "\\u0026")
		.replace(/\u2028/g, "\\u2028")
		.replace(/\u2029/g, "\\u2029");
}

export function buildDeclarationScriptSource(buildId: string): string {
	return [
		inlineBootstrapFallback(buildId),
		`window.__XP_WEB_BUILD_ID__=${serializeInlineScriptString(buildId)};`,
		"if (navigator.serviceWorker?.controller) {",
		"navigator.serviceWorker.controller.postMessage({",
		'type: "XP_DECLARE_BUILD",',
		"buildId: window.__XP_WEB_BUILD_ID__",
		"});",
		"}",
	].join("");
}

function buildHint(buildId: string): string {
	return `xp-build=${encodeURIComponent(buildId)}`;
}

function withBuildHint(url: string, buildId: string): string {
	return `${url}${url.includes("?") ? "&" : "?"}${buildHint(buildId)}`;
}

export function transformIndexHtmlWithInlineBuildDeclaration(
	html: string,
	buildId: string,
): string {
	const declaration = `<script>${buildDeclarationScriptSource(buildId)}</script>`;
	return html
		.replace(/src="([^\"]+\.(?:js|tsx))"/, (_match, url: string) => {
			return `src="${withBuildHint(url, buildId)}"`;
		})
		.replace(/<link\b[^>]*>/g, (tag) => {
			if (!/\brel="stylesheet"/.test(tag)) return tag;
			return tag.replace(
				/href="([^\"]+\.css(?:\?[^\"]*)?)"/,
				(_match, url: string) => `href="${withBuildHint(url, buildId)}"`,
			);
		})
		.replace("<head>", `<head>${declaration}`);
}

export function transformIndexHtmlWithExternalBuildDeclaration(
	html: string,
	buildId: string,
	declarationUrl: string,
): string {
	return html
		.replace(/src="([^\"]+\.(?:js|tsx))"/, (_match, url: string) => {
			return `src="${withBuildHint(url, buildId)}"`;
		})
		.replace(/<link\b[^>]*>/g, (tag) => {
			if (!/\brel="stylesheet"/.test(tag)) return tag;
			return tag.replace(
				/href="([^\"]+\.css(?:\?[^\"]*)?)"/,
				(_match, url: string) => `href="${withBuildHint(url, buildId)}"`,
			);
		})
		.replace("<head>", `<head><script src="${declarationUrl}" defer></script>`);
}
