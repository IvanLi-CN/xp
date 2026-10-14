import { QueryClientProvider } from "@tanstack/react-query";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { RouterProvider } from "@tanstack/react-router";
import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";
import { PrimaryBackendProvider } from "./backend/PrimaryBackendProvider";
import { installPrimaryBackendTransport } from "./backend/primaryBackend";
import {
	DocumentFallbackBoundary,
	FrameworkErrorBoundary,
	installDocumentFallbackHandlers,
} from "./components/FrameworkErrorBoundary";
import { UiPrefsProvider } from "./components/UiPrefs";
import { isDemoRuntime } from "./demo/runtimeMode";
import { AppRuntimeProvider } from "./offline/appRuntime";
import { installOfflineApiWriteGuard } from "./offline/installOfflineApiWriteGuard";
import { createPersistOptions } from "./offline/queryPersistence";
import { declareServiceWorkerBuild } from "./offline/serviceWorkerBuild";
import { createQueryClient } from "./queryClient";
import { createAppRouter } from "./router";
import { renderDocumentFallback } from "./runtime/documentFallback";

async function recoverDemoFromLegacyServiceWorker(): Promise<boolean> {
	if (
		(typeof __XP_WEB_DEMO__ !== "undefined" && __XP_WEB_DEMO__) ||
		typeof window === "undefined" ||
		!/^\/demo(?:\/|$)/.test(window.location.pathname) ||
		!("serviceWorker" in navigator)
	)
		return false;

	// A live XP shell at a Demo URL indicates legacy navigation interception.
	// A correctly served standalone Demo must preserve existing live workers.
	const controller = navigator.serviceWorker.controller;
	if (!controller) return false;
	const registration = await navigator.serviceWorker.getRegistration(
		window.location.href,
	);
	if (registration?.active?.scriptURL !== controller.scriptURL) return false;
	await registration.unregister();
	const url = new URL(window.location.href);
	url.searchParams.set("__xp_demo_sw_recovery", String(Date.now()));
	window.location.replace(url.toString());
	return true;
}

async function bootstrap() {
	if (await recoverDemoFromLegacyServiceWorker()) return;
	const rootElement = document.getElementById("root");
	if (!rootElement) {
		throw new Error("Root element not found");
	}

	installDocumentFallbackHandlers(rootElement);
	const demo = isDemoRuntime();
	if (demo) {
		const { bootstrapDemoRuntime } = await import("./demo/runtime");
		await bootstrapDemoRuntime();
	} else {
		installPrimaryBackendTransport();
		installOfflineApiWriteGuard();
		declareServiceWorkerBuild();
	}

	const queryClient = createQueryClient();
	const router = createAppRouter();
	const reactRoot = ReactDOM.createRoot(rootElement);
	const application = (
		<UiPrefsProvider>
			<AppRuntimeProvider>
				<PrimaryBackendProvider>
					<RouterProvider router={router} />
				</PrimaryBackendProvider>
			</AppRuntimeProvider>
		</UiPrefsProvider>
	);

	reactRoot.render(
		<React.StrictMode>
			<DocumentFallbackBoundary>
				<FrameworkErrorBoundary>
					{demo ? (
						<QueryClientProvider client={queryClient}>
							{application}
						</QueryClientProvider>
					) : (
						<PersistQueryClientProvider
							client={queryClient}
							persistOptions={createPersistOptions()}
						>
							{application}
						</PersistQueryClientProvider>
					)}
				</FrameworkErrorBoundary>
			</DocumentFallbackBoundary>
		</React.StrictMode>,
	);
}

void bootstrap().catch(renderDocumentFallback);
