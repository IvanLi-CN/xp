import type { Meta, StoryObj } from "@storybook/react";

import { QueryRefreshError } from "./QueryRefreshError";

const meta = {
	title: "Components/QueryRefreshError",
	component: QueryRefreshError,
	tags: ["autodocs", "coverage-ui"],
	args: {
		title: "Traffic refresh failed",
		description: "503 upstream unavailable",
		error: new Error("upstream unavailable"),
		onRetry: () => {},
	},
} satisfies Meta<typeof QueryRefreshError>;

export default meta;

type Story = StoryObj<typeof meta>;

export const CachedReportRetry: Story = {};
