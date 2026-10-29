import type { Preview } from "@storybook/react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
	Outlet,
	RouterProvider,
	createMemoryHistory,
	createRootRoute,
	createRoute,
	createRouter,
} from "@tanstack/react-router";

import { PrimaryBackendProvider } from "../src/backend/PrimaryBackendProvider";
import { AppLayout } from "../src/components/AppLayout";
import { PwaStatusPrompt } from "../src/components/PwaStatusPrompt";
import { ToastProvider } from "../src/components/Toast";
import { UiPrefsProvider } from "../src/components/UiPrefs";
import {
	UI_DENSITY_STORAGE_KEY,
	UI_THEME_STORAGE_KEY,
} from "../src/components/UiPrefs";
import { clearAdminToken, writeAdminToken } from "../src/components/auth";
import { AppRuntimeProvider } from "../src/offline/appRuntime";
import { createQueryClient } from "../src/queryClient";
import { EndpointDetailsPage } from "../src/views/EndpointDetailsPage";
import { EndpointNewPage } from "../src/views/EndpointNewPage";
import { EndpointProbeRunPage } from "../src/views/EndpointProbeRunPage";
import { EndpointProbeStatsPage } from "../src/views/EndpointProbeStatsPage";
import { EndpointsPage } from "../src/views/EndpointsPage";
import { HomePage } from "../src/views/HomePage";
import { LoginPage } from "../src/views/LoginPage";
import { NodeDetailsPage } from "../src/views/NodeDetailsPage";
import { NodesPage } from "../src/views/NodesPage";
import { QuotaPolicyPage } from "../src/views/QuotaPolicyPage";
import { ServiceConfigPage } from "../src/views/ServiceConfigPage";
import { ServiceMonitorDetailsPage } from "../src/views/ServiceMonitorDetailsPage";
import {
	ServiceMonitorEditPage,
	ServiceMonitorNewPage,
} from "../src/views/ServiceMonitorEditorPage";
import { ServiceMonitorsPage } from "../src/views/ServiceMonitorsPage";
import { ToolsPage } from "../src/views/ToolsPage";
import { UserDetailsPage } from "../src/views/UserDetailsPage";
import { UserNewPage } from "../src/views/UserNewPage";
import { UsersPage } from "../src/views/UsersPage";

import "../src/styles.css";
import "./docs-theme.css";
import {
	type StorybookApiMockConfig,
	configureStorybookApiMock,
	installStorybookFetchMock,
} from "./mocks/apiMock";

type StorybookRouterParameters = {
	initialEntry?: string;
};

type StorybookAppRuntimeParameters = {
	initialIsOnline?: boolean;
};

installStorybookFetchMock();

function safeLocalStorageSet(key: string, value: string) {
	try {
		localStorage.setItem(key, value);
	} catch {
		// ignore
	}
}

function safeLocalStorageRemove(key: string) {
	try {
		localStorage.removeItem(key);
	} catch {
		// ignore
	}
}

function clearStorybookNodeDeleteOperations() {
	try {
		const prefix = "xp_node_delete_operation_v1:";
		for (let index = sessionStorage.length - 1; index >= 0; index -= 1) {
			const key = sessionStorage.key(index);
			if (key?.startsWith(prefix)) {
				sessionStorage.removeItem(key);
			}
		}
	} catch {
		// ignore
	}
}

export const globalTypes = {
	theme: {
		name: "Theme",
		description: "UI theme used by UiPrefsProvider",
		toolbar: {
			icon: "circlehollow",
			dynamicTitle: true,
			items: [
				{ value: "dark", title: "dark" },
				{ value: "light", title: "light" },
			],
		},
	},
	density: {
		name: "Density",
		description: "UI density used by UiPrefsProvider",
		toolbar: {
			icon: "compress",
			dynamicTitle: true,
			items: [
				{ value: "comfortable", title: "comfortable" },
				{ value: "compact", title: "compact" },
			],
		},
	},
} as const;

export const initialGlobals = {
	theme: "dark",
	density: "comfortable",
} as const;

const preview: Preview = {
	tags: ["autodocs"],
	parameters: {
		actions: { argTypesRegex: "^on[A-Z].*" },
		controls: {
			matchers: {
				color: /(background|color)$/i,
				date: /Date$/,
			},
		},
		docs: {
			toc: true,
		},
	},
	decorators: [
		(Story, context) => {
			const queryClient = createQueryClient();
			const mockParams =
				(context.parameters?.mockApi as StorybookApiMockConfig | undefined) ??
				undefined;
			const routerParams =
				(context.parameters?.router as StorybookRouterParameters | undefined) ??
				undefined;
			const appRuntimeParams =
				(context.parameters?.appRuntime as
					| StorybookAppRuntimeParameters
					| undefined) ?? undefined;
			const theme = context.globals.theme === "light" ? "light" : "dark";
			const density =
				context.globals.density === "compact" ? "compact" : "comfortable";

			configureStorybookApiMock(context.id, mockParams);

			safeLocalStorageSet(UI_THEME_STORAGE_KEY, theme);
			safeLocalStorageSet(UI_DENSITY_STORAGE_KEY, density);

			if (mockParams?.adminToken === null) {
				clearAdminToken();
			} else {
				writeAdminToken(mockParams?.adminToken ?? "storybook-admin-token");
			}
			clearStorybookNodeDeleteOperations();

			const history = createMemoryHistory({
				initialEntries: [routerParams?.initialEntry ?? "/__story"],
			});

			const StoryRoute = () => <Story />;

			const rootRoute = createRootRoute({
				component: RootLayout,
			});

			const storyRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/__story",
				component: StoryRoute,
			});

			const loginRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/login",
				component: LoginPage,
			});

			const appRoute = createRoute({
				getParentRoute: () => rootRoute,
				id: "app",
				component: AppLayout,
			});

			const dashboardRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/",
				component: HomePage,
			});

			const nodesRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/nodes",
				component: NodesPage,
			});

			const nodesJoinRoute = createRoute({
				getParentRoute: () => nodesRoute,
				path: "join",
				component: () => null,
			});

			const nodesRepositoriesRoute = createRoute({
				getParentRoute: () => nodesRoute,
				path: "repositories",
				component: () => null,
			});

			const nodeDetailsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/nodes/$nodeId",
				component: NodeDetailsPage,
			});

			const endpointsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/endpoints",
				component: EndpointsPage,
			});

			const endpointNewRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/endpoints/new",
				component: EndpointNewPage,
			});

			const endpointDetailsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/endpoints/$endpointId",
				component: EndpointDetailsPage,
			});

			const endpointProbeRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/endpoints/$endpointId/probe",
				component: EndpointProbeStatsPage,
			});

			const endpointProbeRunRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/endpoints/probe/runs/$runId",
				component: EndpointProbeRunPage,
			});

			const serviceMonitorsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/monitors",
				component: ServiceMonitorsPage,
			});

			const serviceMonitorNewRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/monitors/new",
				component: ServiceMonitorNewPage,
			});

			const serviceMonitorDetailsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/monitors/$monitorId",
				component: ServiceMonitorDetailsPage,
			});

			const serviceMonitorEditRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/monitors/$monitorId/edit",
				component: ServiceMonitorEditPage,
			});

			const usersRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/users",
				component: UsersPage,
			});

			const userNewRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/users/new",
				component: UserNewPage,
			});

			const userDetailsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/users/$userId",
				component: UserDetailsPage,
			});

			const quotaPolicyRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/quota-policy",
				component: QuotaPolicyPage,
			});
			const serviceConfigRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/service-config",
				component: ServiceConfigPage,
			});
			const toolsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/tools",
				component: ToolsPage,
			});

			const nodesRouteTree = nodesRoute.addChildren([
				nodesJoinRoute,
				nodesRepositoriesRoute,
			]);

			const appRouteTree = appRoute.addChildren([
				dashboardRoute,
				nodesRouteTree,
				nodeDetailsRoute,
				endpointsRoute,
				endpointNewRoute,
				endpointDetailsRoute,
				endpointProbeRoute,
				endpointProbeRunRoute,
				serviceMonitorsRoute,
				serviceMonitorNewRoute,
				serviceMonitorDetailsRoute,
				serviceMonitorEditRoute,
				usersRoute,
				userNewRoute,
				userDetailsRoute,
				quotaPolicyRoute,
				serviceConfigRoute,
				toolsRoute,
			]);

			const routeTree = rootRoute.addChildren([
				storyRoute,
				loginRoute,
				appRouteTree,
			]);

			const router = createRouter({ routeTree, history });

			return (
				<QueryClientProvider client={queryClient}>
					<UiPrefsProvider key={`${theme}-${density}`}>
						<AppRuntimeProvider
							initialIsOnline={appRuntimeParams?.initialIsOnline}
						>
							<ToastProvider>
								<PrimaryBackendProvider>
									<RouterProvider router={router} />
								</PrimaryBackendProvider>
								<PwaStatusPrompt />
							</ToastProvider>
						</AppRuntimeProvider>
					</UiPrefsProvider>
				</QueryClientProvider>
			);
		},
	],
};

export default preview;

function RootLayout() {
	return <Outlet />;
}
