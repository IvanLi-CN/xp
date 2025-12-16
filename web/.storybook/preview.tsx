import type { Preview } from "@storybook/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import React from "react";

import "../src/styles.css";
import { createQueryClient } from "../src/queryClient";
import { createAppRouter } from "../src/router";

const preview: Preview = {
	decorators: [
		(Story) => {
			const queryClient = createQueryClient();
			const router = createAppRouter();

			return (
				<QueryClientProvider client={queryClient}>
					<RouterProvider router={router}>
						<Story />
					</RouterProvider>
				</QueryClientProvider>
			);
		},
	],
};

export default preview;
