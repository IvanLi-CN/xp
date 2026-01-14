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

import { AppLayout } from "../src/components/AppLayout";
import { ToastProvider } from "../src/components/Toast";
import { UiPrefsProvider } from "../src/components/UiPrefs";
import {
	UI_DENSITY_STORAGE_KEY,
	UI_THEME_STORAGE_KEY,
} from "../src/components/UiPrefs";
import { clearAdminToken, writeAdminToken } from "../src/components/auth";
import { createQueryClient } from "../src/queryClient";
import { EndpointDetailsPage } from "../src/views/EndpointDetailsPage";
import { EndpointNewPage } from "../src/views/EndpointNewPage";
import { EndpointsPage } from "../src/views/EndpointsPage";
import { GrantDetailsPage } from "../src/views/GrantDetailsPage";
import { GrantNewPage } from "../src/views/GrantNewPage";
import { GrantsPage } from "../src/views/GrantsPage";
import { HomePage } from "../src/views/HomePage";
import { LoginPage } from "../src/views/LoginPage";
import { NodeDetailsPage } from "../src/views/NodeDetailsPage";
import { NodesPage } from "../src/views/NodesPage";
import { UserDetailsPage } from "../src/views/UserDetailsPage";
import { UserNewPage } from "../src/views/UserNewPage";
import { UsersPage } from "../src/views/UsersPage";

import "../src/styles.css";
import {
	type StorybookApiMockConfig,
	configureStorybookApiMock,
	installStorybookFetchMock,
} from "./mocks/apiMock";

type StorybookRouterParameters = {
	initialEntry?: string;
};

type StorybookUiParameters = {
	theme?: "system" | "light" | "dark";
	density?: "comfortable" | "compact";
};

installStorybookFetchMock();

function safeLocalStorageSet(key: string, value: string) {
	try {
		localStorage.setItem(key, value);
	} catch {
		// ignore
	}
}

const preview: Preview = {
	decorators: [
		(Story, context) => {
			const queryClient = createQueryClient();
			const mockParams =
				(context.parameters?.mockApi as StorybookApiMockConfig | undefined) ??
				undefined;
			const routerParams =
				(context.parameters?.router as StorybookRouterParameters | undefined) ??
				undefined;
			const uiParams =
				(context.parameters?.ui as StorybookUiParameters | undefined) ??
				undefined;

			configureStorybookApiMock(context.id, mockParams);

			if (uiParams?.theme) {
				safeLocalStorageSet(UI_THEME_STORAGE_KEY, uiParams.theme);
			}
			if (uiParams?.density) {
				safeLocalStorageSet(UI_DENSITY_STORAGE_KEY, uiParams.density);
			}

			if (mockParams?.adminToken === null) {
				clearAdminToken();
			} else {
				writeAdminToken(mockParams?.adminToken ?? "storybook-admin-token");
			}

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

			const grantsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/grants",
				component: GrantsPage,
			});

			const grantNewRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/grants/new",
				component: GrantNewPage,
			});

			const grantDetailsRoute = createRoute({
				getParentRoute: () => appRoute,
				path: "/grants/$grantId",
				component: GrantDetailsPage,
			});

			const appRouteTree = appRoute.addChildren([
				dashboardRoute,
				nodesRoute,
				nodeDetailsRoute,
				endpointsRoute,
				endpointNewRoute,
				endpointDetailsRoute,
				usersRoute,
				userNewRoute,
				userDetailsRoute,
				grantsRoute,
				grantNewRoute,
				grantDetailsRoute,
			]);

			const routeTree = rootRoute.addChildren([
				storyRoute,
				loginRoute,
				appRouteTree,
			]);

			const router = createRouter({ routeTree, history });

			return (
				<QueryClientProvider client={queryClient}>
					<UiPrefsProvider>
						<ToastProvider>
							<RouterProvider router={router} />
						</ToastProvider>
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
