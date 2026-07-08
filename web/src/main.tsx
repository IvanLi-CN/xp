import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { RouterProvider } from "@tanstack/react-router";
import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";
import { UiPrefsProvider } from "./components/UiPrefs";
import { AppRuntimeProvider } from "./offline/appRuntime";
import { installOfflineApiWriteGuard } from "./offline/installOfflineApiWriteGuard";
import { createPersistOptions } from "./offline/queryPersistence";
import { createQueryClient } from "./queryClient";
import { createAppRouter } from "./router";

const queryClient = createQueryClient();
const router = createAppRouter();

installOfflineApiWriteGuard();

const rootElement = document.getElementById("root");
if (!rootElement) {
	throw new Error("Root element not found");
}

ReactDOM.createRoot(rootElement).render(
	<React.StrictMode>
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
	</React.StrictMode>,
);
