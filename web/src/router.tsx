import {
	Outlet,
	createRootRoute,
	createRoute,
	createRouter,
	lazyRouteComponent,
	redirect,
	retainSearchParams,
} from "@tanstack/react-router";
import { DemoFrame } from "@xp/demo-frame";
import { z } from "zod";

import { AppLayout } from "./components/AppLayout";
import { AuthGate } from "./components/AuthGate";
import { FrameworkErrorRecovery } from "./components/FrameworkErrorRecovery";
import { PwaStatusPrompt } from "./components/PwaStatusPrompt";
import { ToastProvider } from "./components/Toast";
import { hasAdminToken } from "./components/auth";
import { isDemoRuntime } from "./demo/runtimeMode";
import { DEMO_SHARE_KEYS } from "./demo/shareState";

import {
	buildRedirectTarget,
	parseLoginHandoffHash,
	resolveLoginRedirectFromHref,
} from "./utils/navigation";
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
import { ServiceMonitorDetailsPage } from "./views/ServiceMonitorDetailsPage";
import {
	ServiceMonitorEditPage,
	ServiceMonitorNewPage,
} from "./views/ServiceMonitorEditorPage";
import { ServiceMonitorsPage } from "./views/ServiceMonitorsPage";
import { SystemStatusPage } from "./views/SystemStatusPage";
import { ToolsPage } from "./views/ToolsPage";
import { UserDetailsPage } from "./views/UserDetailsPage";
import { UserNewPage } from "./views/UserNewPage";
import { UsersPage } from "./views/UsersPage";
import { NODE_DETAILS_TAB_VALUES } from "./views/nodeDetailsTabs";

const UiDemoSystemStatusPage = lazyRouteComponent(
	() => import("./demo/DemoSystemStatusPage"),
	"UiDemoSystemStatusPage",
);
const UiDemoFrameworkRecoveryPage = lazyRouteComponent(
	() => import("./demo/DemoFrameworkRecoveryPage"),
	"DemoFrameworkRecoveryPage",
);
const rootRoute = createRootRoute({
	component: RootLayout,
	validateSearch: z.record(z.string(), z.string()).catch({}),
	search: {
		middlewares: [
			retainSearchParams(isDemoRuntime() ? [...DEMO_SHARE_KEYS] : []),
		],
	},
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
		if (isDemoRuntime()) return;
		const targetUrl = new URL(location.href, window.location.origin);
		const handoff = parseLoginHandoffHash(targetUrl.hash);
		if (hasAdminToken()) {
			if (handoff) {
				window.history.replaceState(
					null,
					"",
					buildRedirectTarget(
						targetUrl.pathname,
						targetUrl.search,
						handoff.redirectHash,
					),
				);
			}
			return;
		}
		if (!hasAdminToken()) {
			const loginSearch = new URLSearchParams(targetUrl.search);
			const loginToken = loginSearch.get("login_token") ?? handoff?.token;
			const redirectTarget = resolveLoginRedirectFromHref(location.href);
			throw redirect({
				replace: true,
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
	validateSearch: z
		.object({
			tab: z.enum(NODE_DETAILS_TAB_VALUES).optional(),
		})
		.passthrough()
		.catch({}),
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
	systemStatusRoute,
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
	loginRoute,
	uiDemoSystemStatusRoute,
	uiDemoFrameworkRecoveryRoute,
	appRouteTree,
]);

export function createAppRouter() {
	const router = createRouter({
		routeTree,
		basepath: isDemoRuntime() ? "/demo" : "/",
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
	if (isDemoRuntime())
		return (
			<DemoFrame>
				<Outlet />
			</DemoFrame>
		);
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
