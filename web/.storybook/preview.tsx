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
import React from "react";

import "../src/styles.css";
import { createQueryClient } from "../src/queryClient";

const preview: Preview = {
	decorators: [
		(Story) => {
			const queryClient = createQueryClient();
			const history = createMemoryHistory({
				initialEntries: ["/"],
			});

			const rootRoute = createRootRoute({
				component: () => <Outlet />,
			});

			const storyRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/",
				component: () => <Story />,
			});

			const nodesRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/nodes",
				component: () => null,
			});

			const endpointsRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/endpoints",
				component: () => null,
			});

			const usersRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/users",
				component: () => null,
			});

			const grantsRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/grants",
				component: () => null,
			});

			const loginRoute = createRoute({
				getParentRoute: () => rootRoute,
				path: "/login",
				component: () => null,
			});

			const routeTree = rootRoute.addChildren([
				storyRoute,
				nodesRoute,
				endpointsRoute,
				usersRoute,
				grantsRoute,
				loginRoute,
			]);

			const router = createRouter({ routeTree, history });

			return (
				<QueryClientProvider client={queryClient}>
					<RouterProvider router={router} />
				</QueryClientProvider>
			);
		},
	],
};

export default preview;
