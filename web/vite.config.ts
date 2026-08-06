import { createHash } from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";
import { VitePWA } from "vite-plugin-pwa";

import { transformIndexHtmlWithInlineBuildDeclaration } from "./src/runtime/inlineBuildDeclaration";

const packageJson = JSON.parse(
	fs.readFileSync(path.resolve(__dirname, "./package.json"), "utf8"),
) as { version: string };

function resolveBuildId() {
	const explicit = process.env.XP_WEB_BUILD_ID?.trim();
	if (explicit) return explicit;

	const hash = createHash("sha256");
	const ignoredDirectories = new Set([
		"coverage",
		"dist",
		"node_modules",
		"playwright-report",
		"storybook-static",
		"test-results",
	]);
	const visit = (directory: string) => {
		for (const entry of fs
			.readdirSync(directory, { withFileTypes: true })
			.sort((left, right) => left.name.localeCompare(right.name))) {
			if (ignoredDirectories.has(entry.name)) continue;
			const filePath = path.join(directory, entry.name);
			if (entry.isDirectory()) {
				visit(filePath);
				continue;
			}
			if (!entry.isFile()) continue;
			hash.update(path.relative(__dirname, filePath)).update("\0");
			hash.update(fs.readFileSync(filePath));
		}
	};
	visit(__dirname);
	return `${packageJson.version}-${hash.digest("hex").slice(0, 12)}`;
}

function resolveSwUpdateIntervalMs() {
	const raw = process.env.XP_WEB_SW_UPDATE_INTERVAL_MS?.trim();
	if (!raw) return 60_000;

	const parsed = Number(raw);
	if (!Number.isFinite(parsed) || parsed < 0) return 60_000;
	return Math.floor(parsed);
}

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, process.cwd(), "VITE_");
	const backend = env.VITE_BACKEND_PROXY ?? "http://127.0.0.1:62416";
	const buildId = resolveBuildId();
	const swUpdateIntervalMs = resolveSwUpdateIntervalMs();

	return {
		define: {
			__XP_WEB_BUILD_ID__: JSON.stringify(buildId),
			__XP_WEB_PACKAGE_VERSION__: JSON.stringify(packageJson.version),
			__XP_WEB_SW_UPDATE_INTERVAL_MS__: JSON.stringify(swUpdateIntervalMs),
		},
		plugins: [
			react(),
			tailwindcss(),
			{
				name: "xp-inline-build-declaration",
				transformIndexHtml(html, context) {
					if (context.path.endsWith("/iframe.html")) return html;
					return transformIndexHtmlWithInlineBuildDeclaration(html, buildId);
				},
				configurePreviewServer(server) {
					if (process.env.E2E_USE_PREVIEW !== "1") return;
					server.middlewares.use((request, response, next) => {
						const requestUrl = request.url
							? new URL(request.url, "http://127.0.0.1")
							: null;
						if (requestUrl?.pathname === "/__e2e_legacy_worker__.js") {
							response.statusCode = 200;
							response.setHeader("Content-Type", "application/javascript");
							response.setHeader("Cache-Control", "no-store");
							response.end(
								[
									"self.addEventListener('install', (event) => event.waitUntil(self.skipWaiting()));",
									"self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));",
								].join("\n"),
							);
							return;
						}
						if (requestUrl?.pathname === "/__e2e_legacy_client__.html") {
							response.statusCode = 200;
							response.setHeader("Content-Type", "text/html");
							response.setHeader("Cache-Control", "no-store");
							response.end(
								"<!doctype html><title>legacy xp client</title><main>legacy xp client</main>",
							);
							return;
						}
						const waitingToken = requestUrl?.searchParams.get("e2e-waiting");
						const migrationToken = requestUrl?.searchParams.get(
							"e2e-legacy-migration",
						);
						const predecessorToken = requestUrl?.searchParams.get(
							"e2e-legacy-predecessor",
						);
						const failedMigrationToken = requestUrl?.searchParams.get(
							"e2e-legacy-install-failure",
						);
						if (
							requestUrl?.pathname !== "/sw.js" ||
							(!waitingToken &&
								!migrationToken &&
								!predecessorToken &&
								!failedMigrationToken)
						) {
							next();
							return;
						}
						const serviceWorkerPath = path.resolve(__dirname, "dist/sw.js");
						const source = fs.readFileSync(serviceWorkerPath, "utf8");
						const withBuildId = (replacementBuildId: string) =>
							source.replaceAll(
								JSON.stringify(buildId),
								JSON.stringify(replacementBuildId),
							);
						const withoutMigrationActivation = () => {
							const replacement = source.replace(
								/if\([^)]*==="legacy_migration"\)\{/,
								"if(false){",
							);
							if (replacement === source) {
								throw new Error(
									"E2E predecessor fixture could not disable legacy migration activation",
								);
							}
							return replacement;
						};
						const legacyInstallFailureInterceptor = [
							"const __xpE2eFetch = self.fetch.bind(self);",
							"self.fetch = (input, init) => {",
							"const url = input instanceof Request",
							"? input.url",
							": new URL(input, self.registration.scope).href;",
							'if (new URL(url).pathname.endsWith("/site.webmanifest")) {',
							'return Promise.reject(new Error("e2e legacy precache failure"));',
							"}",
							"return __xpE2eFetch(input, init);",
							"};",
						].join(" ");
						const legacyInstallFailureSource = (workerToken: string) =>
							[
								legacyInstallFailureInterceptor,
								source,
								`/* e2e legacy failure ${workerToken} */`,
							].join("\n");
						const body = waitingToken
							? `${withBuildId(`e2e-waiting-${waitingToken}`)}\n/* e2e waiting revision */`
							: predecessorToken
								? `${withoutMigrationActivation().replaceAll(
										JSON.stringify(buildId),
										JSON.stringify(
											`e2e-legacy-predecessor-${predecessorToken}`,
										),
									)}\n/* e2e legacy predecessor ${predecessorToken} */`
								: failedMigrationToken
									? legacyInstallFailureSource(failedMigrationToken)
									: `${source}\n/* e2e legacy migration ${migrationToken} */`;
						response.statusCode = 200;
						response.setHeader("Content-Type", "application/javascript");
						response.setHeader("Cache-Control", "no-store");
						response.end(body);
					});
				},
			},
			VitePWA({
				registerType: "prompt",
				injectRegister: false,
				strategies: "injectManifest",
				srcDir: "src",
				filename: "sw.ts",
				includeAssets: [
					"favicon.ico",
					"favicon-16x16.png",
					"favicon-32x32.png",
					"apple-touch-icon.png",
					"xp-mark.png",
				],
				manifest: {
					name: "xp",
					short_name: "xp",
					start_url: "/",
					scope: "/",
					display: "standalone",
					background_color: "#ffffff",
					theme_color: "#00A9C7",
					icons: [
						{
							src: "/android-chrome-192x192.png",
							sizes: "192x192",
							type: "image/png",
						},
						{
							src: "/android-chrome-512x512.png",
							sizes: "512x512",
							type: "image/png",
						},
					],
				},
				injectManifest: {
					globPatterns: ["**/*.{js,css,html,ico,png,svg,woff2,webmanifest}"],
					maximumFileSizeToCacheInBytes: 6 * 1024 * 1024,
					sourcemap: true,
				},
			}),
		],
		resolve: {
			alias: {
				"@": path.resolve(__dirname, "./src"),
			},
		},
		server: {
			host: "127.0.0.1",
			port: 60080,
			strictPort: true,
			proxy: {
				"/api": { target: backend, changeOrigin: true, secure: false },
				"/events": { target: backend, changeOrigin: true, secure: false },
			},
		},
	};
});
