type DocumentFallbackOptions = {
	buildId?: string;
	path?: string;
	onReload?: () => void;
};

function escapeHtml(value: string): string {
	return value.replace(
		/[&<>'"]/g,
		(character) =>
			({
				"&": "&amp;",
				"<": "&lt;",
				">": "&gt;",
				"'": "&#39;",
				'"': "&quot;",
			})[character] ?? character,
	);
}

function errorMessage(error: unknown): string {
	if (error instanceof Error) return error.message;
	if (typeof error === "string") return error;
	try {
		return JSON.stringify(error);
	} catch {
		return String(error);
	}
}

function redactFallbackText(value: string): string {
	return value
		.replace(/(authorization\s*:\s*bearer\s+)[^\s,]+/gi, "$1[REDACTED]")
		.replace(
			/([?&](?:token|login_token|access_token|api_key|apikey|secret|password|key)=)[^&#\s]+/gi,
			"$1[REDACTED]",
		)
		.replace(
			/((?:token|login_token|access_token|api_key|apikey|secret|password|key)\s*[:=]\s*)[^\s,]+/gi,
			"$1[REDACTED]",
		)
		.replace(
			/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g,
			"[REDACTED]",
		)
		.replace(/\bBearer\s+[^\s,]+/gi, "Bearer [REDACTED]");
}

function diagnosticPath(path: string): string {
	try {
		return new URL(path, "http://xp.invalid").pathname;
	} catch {
		return path.split(/[?#]/, 1)[0] || "/";
	}
}

function reloadPage(): void {
	if (typeof window !== "undefined") window.location.reload();
}

export function renderDocumentFallback(
	error: unknown,
	options: DocumentFallbackOptions = {},
): void {
	if (typeof document === "undefined") return;

	const target = document.getElementById("root") ?? document.body;
	if (!target) return;

	const buildId = escapeHtml(redactFallbackText(options.buildId ?? "unknown"));
	const path = escapeHtml(
		diagnosticPath(
			options.path ??
				(typeof window === "undefined" ? "/" : window.location.pathname),
		),
	);
	const message = escapeHtml(
		redactFallbackText(errorMessage(error)).slice(0, 500),
	);

	target.innerHTML = `
		<style>
			[data-xp-document-fallback] {
				min-height: 100vh;
				box-sizing: border-box;
				display: grid;
				place-items: center;
				padding: 24px;
				background: #f5fbfc;
				color: #16313b;
				font: 16px/1.5 system-ui, sans-serif;
			}
			[data-xp-document-fallback] article {
				width: min(100%, 560px);
				box-sizing: border-box;
				padding: 32px;
				border: 1px solid #c7dfe4;
				border-radius: 20px;
				background: #fff;
				box-shadow: 0 18px 50px rgba(22, 49, 59, 0.12);
			}
			[data-xp-document-fallback] h1 { margin: 0 0 12px; font-size: 28px; line-height: 1.15; }
			[data-xp-document-fallback] p { margin: 0 0 20px; color: #4b6871; }
			[data-xp-document-fallback] button {
				min-height: 44px;
				border: 0;
				border-radius: 10px;
				padding: 0 18px;
				background: #079bb5;
				color: #fff;
				font: inherit;
				font-weight: 700;
				cursor: pointer;
			}
			[data-xp-document-fallback] details { margin-top: 24px; color: #4b6871; font-size: 13px; }
			[data-xp-document-fallback] pre {
				overflow: auto;
				white-space: pre-wrap;
				overflow-wrap: anywhere;
				margin: 10px 0 0;
				padding: 12px;
				border-radius: 10px;
				background: #eef6f7;
				color: #27434c;
			}
			@media (max-width: 640px) {
				[data-xp-document-fallback] { padding: 16px; }
				[data-xp-document-fallback] article {
					padding: 24px;
					border-radius: 16px;
				}
				[data-xp-document-fallback] h1 { font-size: 24px; }
				[data-xp-document-fallback] button { width: 100%; }
			}
		</style>
		<main data-xp-document-fallback>
			<article role="alert">
				<p style="margin-bottom: 8px; color: #087f94; font-weight: 700;">xp</p>
				<h1>xp could not start</h1>
				<p>
					The app hit a startup problem. Reload the page to try again. Your sign-in
					and saved preferences are not cleared.
				</p>
				<button type="button" data-action="reload">Reload app</button>
				<details>
					<summary>Technical details</summary>
					<pre>build: ${buildId}
					path: ${path}
					error: ${message}</pre>
				</details>
			</article>
		</main>
	`;

	const reloadButton = target.querySelector<HTMLButtonElement>(
		"[data-action=reload]",
	);
	reloadButton?.addEventListener("click", () => {
		(options.onReload ?? reloadPage)();
	});
}
