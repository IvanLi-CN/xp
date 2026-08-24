import {
	Outlet,
	createRootRoute,
	createRoute,
	createRouter,
	lazyRouteComponent,
	redirect,
} from "@tanstack/react-router";

import { AppLayout } from "./components/AppLayout";
import { AuthGate } from "./components/AuthGate";
import { FrameworkErrorRecovery } from "./components/FrameworkErrorRecovery";
import { PwaStatusPrompt } from "./components/PwaStatusPrompt";
import { ToastProvider } from "./components/Toast";
import { hasAdminToken } from "./components/auth";
import { hasDemoSession } from "./demo/session";
import { resolveLoginRedirectFromHref } from "./utils/navigation";
import { EndpointDetailsPage } from "./views/EndpointDetailsPage";
import { EndpointNewPage } from "./views/EndpointNewPage";
import { EndpointProbeRunPage } from "./views/EndpointProbeRunPage";
import { EndpointProbeStatsPage } from "./views/EndpointProbeStatsPage";
import { EndpointsPage } from "./views/EndpointsPage";
import { HomePage } from "./views/HomePage";
import { LoginPage } from "./views/LoginPage";
import { NodeDetailsPage } from "./views/NodeDetailsPage";
import { NodesPage } from "./views/NodesPage";
import { QuotaPolicyPage } from "./views/QuotaPolicyPage";
import { ServiceConfigPage } from "./views/ServiceConfigPage";
import { SystemStatusPage } from "./views/SystemStatusPage";
import { ToolsPage } from "./views/ToolsPage";
import { UserDetailsPage } from "./views/UserDetailsPage";
import { UserNewPage } from "./views/UserNewPage";
import { UsersPage } from "./views/UsersPage";

const DemoDashboardPage = lazyRouteComponent(
	() => import("./demo/DemoDashboardPage"),
	"DemoDashboardPage",
);
const DemoEndpointDetailsPage = lazyRouteComponent(
	() => import("./demo/DemoEndpointsPage"),
	"DemoEndpointDetailsPage",
);
const DemoEndpointFormPage = lazyRouteComponent(
	() => import("./demo/DemoEndpointsPage"),
	"DemoEndpointFormPage",
);
const DemoEndpointProbeRunPage = lazyRouteComponent(
	() => import("./demo/DemoEndpointsPage"),
	"DemoEndpointProbeRunPage",
);
const DemoEndpointProbeStatsPage = lazyRouteComponent(
	() => import("./demo/DemoEndpointsPage"),
	"DemoEndpointProbeStatsPage",
);
const DemoEndpointsPage = lazyRouteComponent(
	() => import("./demo/DemoEndpointsPage"),
	"DemoEndpointsPage",
);
const DemoAppRoute = lazyRouteComponent(
	() => import("./demo/DemoLayout"),
	"DemoAppRoute",
);
const DemoLoginRoute = lazyRouteComponent(
	() => import("./demo/DemoLayout"),
	"DemoLoginRoute",
);
const DemoLoginPage = lazyRouteComponent(
	() => import("./demo/DemoLoginPage"),
	"DemoLoginPage",
);
const DemoNodeDetailsPage = lazyRouteComponent(
	() => import("./demo/DemoNodesPage"),
	"DemoNodeDetailsPage",
);
const DemoNodesPage = lazyRouteComponent(
	() => import("./demo/DemoNodesPage"),
	"DemoNodesPage",
);
const DemoScenariosPage = lazyRouteComponent(
	() => import("./demo/DemoScenariosPage"),
	"DemoScenariosPage",
);
const DemoQuotaPolicyPage = lazyRouteComponent(
	() => import("./demo/DemoSettingsPages"),
	"DemoQuotaPolicyPage",
);
const DemoServiceConfigPage = lazyRouteComponent(
	() => import("./demo/DemoSettingsPages"),
	"DemoServiceConfigPage",
);
const DemoToolsPage = lazyRouteComponent(
	() => import("./demo/DemoSettingsPages"),
	"DemoToolsPage",
);
const DemoSystemStatusPage = lazyRouteComponent(
	() => import("./demo/DemoSystemStatusPage"),
	"DemoSystemStatusPage",
);
const UiDemoSystemStatusPage = lazyRouteComponent(
	() => import("./demo/DemoSystemStatusPage"),
	"UiDemoSystemStatusPage",
);
const UiDemoFrameworkRecoveryPage = lazyRouteComponent(
	() => import("./demo/DemoFrameworkRecoveryPage"),
	"DemoFrameworkRecoveryPage",
);
const DemoUserDetailsPage = lazyRouteComponent(
	() => import("./demo/DemoUsersPage"),
	"DemoUserDetailsPage",
);
const DemoUserFormPage = lazyRouteComponent(
	() => import("./demo/DemoUsersPage"),
	"DemoUserFormPage",
);
const DemoUsersPage = lazyRouteComponent(
	() => import("./demo/DemoUsersPage"),
	"DemoUsersPage",
);

const rootRoute = createRootRoute({
	component: RootLayout,
});

const loginRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/login",
	component: LoginPage,
});

const uiDemoSystemStatusRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/ui-demo/system-status",
	component: UiDemoSystemStatusPage,
});

const uiDemoFrameworkRecoveryRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/ui-demo/framework-recovery",
	component: UiDemoFrameworkRecoveryPage,
});

const appRoute = createRoute({
	getParentRoute: () => rootRoute,
	id: "app",
	beforeLoad: ({ location }) => {
		if (!hasAdminToken()) {
			const targetUrl = new URL(location.href, window.location.origin);
			const loginSearch = new URLSearchParams(targetUrl.search);
			const loginToken = loginSearch.get("login_token");
			const redirectTarget = resolveLoginRedirectFromHref(location.href);
			throw redirect({
				to: "/login",
				search: {
					redirect: redirectTarget,
					...(loginToken ? { login_token: loginToken } : {}),
				},
			});
		}
	},
	component: AppShell,
});

const dashboardRoute = createRoute({
	getParentRoute: () => appRoute,
	path: "/",
	component: HomePage,
});

const systemStatusRoute = createRoute({
	getParentRoute: () => appRoute,
	path: "/system-status",
	component: SystemStatusPage,
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

const demoLoginRootRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/demo/login",
	component: DemoLoginRoute,
});

const demoLoginPageRoute = createRoute({
	getParentRoute: () => demoLoginRootRoute,
	path: "/",
	component: DemoLoginPage,
});

const demoAppRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/demo",
	beforeLoad: () => {
		if (!hasDemoSession()) {
			throw redirect({ to: "/demo/login" });
		}
	},
	component: DemoAppRoute,
});

const demoDashboardRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/",
	component: DemoDashboardPage,
});

const demoSystemStatusRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/system-status",
	component: DemoSystemStatusPage,
});

const demoNodesRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/nodes",
	component: DemoNodesPage,
});

const demoNodesJoinRoute = createRoute({
	getParentRoute: () => demoNodesRoute,
	path: "join",
	component: () => null,
});

const demoNodesRepositoriesRoute = createRoute({
	getParentRoute: () => demoNodesRoute,
	path: "repositories",
	component: () => null,
});

const demoNodeDetailsRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/nodes/$nodeId",
	component: DemoNodeDetailsPage,
});

const demoEndpointsRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/endpoints",
	component: DemoEndpointsPage,
});

const demoEndpointNewRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/endpoints/new",
	component: DemoEndpointFormPage,
});

const demoEndpointDetailsRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/endpoints/$endpointId",
	component: DemoEndpointDetailsPage,
});

const demoEndpointProbeRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/endpoints/$endpointId/probe",
	component: DemoEndpointProbeStatsPage,
});

const demoEndpointProbeRunRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/endpoints/probe/runs/$runId",
	component: DemoEndpointProbeRunPage,
});

const demoUsersRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/users",
	component: DemoUsersPage,
});

const demoUserNewRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/users/new",
	component: DemoUserFormPage,
});

const demoUserDetailsRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/users/$userId",
	component: DemoUserDetailsPage,
});

const demoScenariosRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/scenarios",
	component: DemoScenariosPage,
});

const demoQuotaPolicyRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/quota-policy",
	component: DemoQuotaPolicyPage,
});

const demoServiceConfigRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/service-config",
	component: DemoServiceConfigPage,
});

const demoToolsRoute = createRoute({
	getParentRoute: () => demoAppRoute,
	path: "/tools",
	component: DemoToolsPage,
});

const nodesRouteTree = nodesRoute.addChildren([
	nodesJoinRoute,
	nodesRepositoriesRoute,
]);

const appRouteTree = appRoute.addChildren([
	dashboardRoute,
	systemStatusRoute,
	nodesRouteTree,
	nodeDetailsRoute,
	endpointsRoute,
	endpointNewRoute,
	endpointDetailsRoute,
	endpointProbeRoute,
	endpointProbeRunRoute,
	usersRoute,
	userNewRoute,
	userDetailsRoute,
	quotaPolicyRoute,
	serviceConfigRoute,
	toolsRoute,
]);

const demoLoginRouteTree = demoLoginRootRoute.addChildren([demoLoginPageRoute]);

const demoNodesRouteTree = demoNodesRoute.addChildren([
	demoNodesJoinRoute,
	demoNodesRepositoriesRoute,
]);

const demoAppRouteTree = demoAppRoute.addChildren([
	demoDashboardRoute,
	demoSystemStatusRoute,
	demoNodesRouteTree,
	demoNodeDetailsRoute,
	demoEndpointsRoute,
	demoEndpointNewRoute,
	demoEndpointDetailsRoute,
	demoEndpointProbeRoute,
	demoEndpointProbeRunRoute,
	demoUsersRoute,
	demoUserNewRoute,
	demoUserDetailsRoute,
	demoScenariosRoute,
	demoQuotaPolicyRoute,
	demoServiceConfigRoute,
	demoToolsRoute,
]);

const routeTree = rootRoute.addChildren([
	loginRoute,
	uiDemoSystemStatusRoute,
	uiDemoFrameworkRecoveryRoute,
	appRouteTree,
	demoLoginRouteTree,
	demoAppRouteTree,
]);

export function createAppRouter() {
	const router = createRouter({
		routeTree,
		defaultErrorComponent: ({ error }) => (
			<FrameworkErrorRecovery error={error} />
		),
	});

	return router;
}

declare module "@tanstack/react-router" {
	interface Register {
		router: ReturnType<typeof createAppRouter>;
	}
}

function RootLayout() {
	return (
		<>
			<Outlet />
			<PwaStatusPrompt />
		</>
	);
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
