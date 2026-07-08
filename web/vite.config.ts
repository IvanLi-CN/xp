import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";
import { VitePWA } from "vite-plugin-pwa";

const packageJson = JSON.parse(
	fs.readFileSync(path.resolve(__dirname, "./package.json"), "utf8"),
) as { version: string };

function resolveBuildId() {
	const explicit = process.env.XP_WEB_BUILD_ID?.trim();
	if (explicit) return explicit;

	try {
		const sha = execSync("git rev-parse --short HEAD", {
			cwd: path.resolve(__dirname, ".."),
			stdio: ["ignore", "pipe", "ignore"],
		})
			.toString()
			.trim();
		if (sha) return `${packageJson.version}-${sha}`;
	} catch {
		// ignore and fall back to package version
	}

	return packageJson.version;
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
			VitePWA({
				registerType: "prompt",
				injectRegister: false,
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
				workbox: {
					globPatterns: ["**/*.{js,css,html,ico,png,svg,woff2,webmanifest}"],
					maximumFileSizeToCacheInBytes: 6 * 1024 * 1024,
					navigateFallback: "/index.html",
					navigateFallbackDenylist: [/^\/api\//, /^\/events(?:\/|$)/],
					cleanupOutdatedCaches: true,
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
