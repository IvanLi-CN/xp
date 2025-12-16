import {
	Link,
	Outlet,
	createRootRoute,
	createRoute,
	createRouter,
} from "@tanstack/react-router";

import { HomePage } from "./views/HomePage";

const rootRoute = createRootRoute({
	component: RootLayout,
});

const indexRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: "/",
	component: HomePage,
});

const routeTree = rootRoute.addChildren([indexRoute]);

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
	return (
		<div className="min-h-screen bg-base-200">
			<header className="navbar bg-base-100 shadow">
				<div className="flex-1">
					<Link className="btn btn-ghost text-xl" to="/">
						xp
					</Link>
				</div>
			</header>
			<main className="p-6">
				<Outlet />
			</main>
		</div>
	);
}
