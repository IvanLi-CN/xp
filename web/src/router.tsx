import {
	Outlet,
	createRootRoute,
	createRoute,
	createRouter,
	redirect,
} from "@tanstack/react-router";

import { AppLayout } from "./components/AppLayout";
import { AuthGate } from "./components/AuthGate";
import { ToastProvider } from "./components/Toast";
import { hasAdminToken } from "./components/auth";
import { EndpointDetailsPage } from "./views/EndpointDetailsPage";
import { EndpointNewPage } from "./views/EndpointNewPage";
import { EndpointsPage } from "./views/EndpointsPage";
import { GrantDetailsPage } from "./views/GrantDetailsPage";
import { GrantNewPage } from "./views/GrantNewPage";
import { GrantsPage } from "./views/GrantsPage";
import { HomePage } from "./views/HomePage";
import { LoginPage } from "./views/LoginPage";
import { NodeDetailsPage } from "./views/NodeDetailsPage";
import { NodesPage } from "./views/NodesPage";
import { UserDetailsPage } from "./views/UserDetailsPage";
import { UserNewPage } from "./views/UserNewPage";
import { UsersPage } from "./views/UsersPage";

const rootRoute = createRootRoute({
	component: RootLayout,
});

const loginRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/login",
	component: LoginPage,
});

const appRoute = createRoute({
	getParentRoute: () => rootRoute,
	id: "app",
	beforeLoad: () => {
		if (!hasAdminToken()) {
			throw redirect({ to: "/login" });
		}
	},
	component: AppShell,
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

const routeTree = rootRoute.addChildren([loginRoute, appRouteTree]);

export function createAppRouter() {
	const router = createRouter({ routeTree });

	return router;
}

declare module "@tanstack/react-router" {
	interface Register {
		router: ReturnType<typeof createAppRouter>;
	}
}

function RootLayout() {
	return <Outlet />;
}

function AppShell() {
	return (
		<ToastProvider>
			<AuthGate>
				<AppLayout />
			</AuthGate>
		</ToastProvider>
	);
}
