import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { RouterProvider } from "@tanstack/react-router";
import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";
import {
	DocumentFallbackBoundary,
	FrameworkErrorBoundary,
	installDocumentFallbackHandlers,
} from "./components/FrameworkErrorBoundary";
import { UiPrefsProvider } from "./components/UiPrefs";
import { AppRuntimeProvider } from "./offline/appRuntime";
import { installOfflineApiWriteGuard } from "./offline/installOfflineApiWriteGuard";
import { createPersistOptions } from "./offline/queryPersistence";
import { declareServiceWorkerBuild } from "./offline/serviceWorkerBuild";
import { createQueryClient } from "./queryClient";
import { createAppRouter } from "./router";
import { renderDocumentFallback } from "./runtime/documentFallback";

function bootstrap() {
	const rootElement = document.getElementById("root");
	if (!rootElement) {
		throw new Error("Root element not found");
	}

	installDocumentFallbackHandlers(rootElement);
	installOfflineApiWriteGuard();
	declareServiceWorkerBuild();

	const queryClient = createQueryClient();
	const router = createAppRouter();
	const reactRoot = ReactDOM.createRoot(rootElement);

	reactRoot.render(
		<React.StrictMode>
			<DocumentFallbackBoundary>
				<FrameworkErrorBoundary>
					<PersistQueryClientProvider
						client={queryClient}
						persistOptions={createPersistOptions()}
					>
						<UiPrefsProvider>
							<AppRuntimeProvider>
								<RouterProvider router={router} />
							</AppRuntimeProvider>
						</UiPrefsProvider>
					</PersistQueryClientProvider>
				</FrameworkErrorBoundary>
			</DocumentFallbackBoundary>
		</React.StrictMode>,
	);
}

try {
	bootstrap();
} catch (error) {
	renderDocumentFallback(error);
}
