import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";

import { BackendApiError } from "../api/backendError";
import type { CacheRecoveryResult } from "../runtime/frameworkErrorRecovery";
import { FrameworkErrorRecovery } from "./FrameworkErrorRecovery";

const skippedCacheRecovery = async (): Promise<CacheRecoveryResult> => ({
	status: "skipped",
	reason: "replacement-unavailable",
	deleted: [],
});

const meta = {
	title: "Components/FrameworkErrorRecovery",
	component: FrameworkErrorRecovery,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Root recovery states for framework, asset, cache, offline, and API failures. " +
					"Cache recovery is intentionally guarded by a replacement-ready result.",
			},
		},
	},
} satisfies Meta<typeof FrameworkErrorRecovery>;

export default meta;

type Story = StoryObj<typeof meta>;

function expectReloadAction(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	return expect(
		canvas.getByRole("button", { name: "Reload app" }),
	).toBeInTheDocument();
}

export const FirstFailureChunkLoad: Story = {
	args: {
		error: new Error("Failed to fetch dynamically imported module"),
		category: "chunk-load",
		onReload: fn(),
		onClearCachedApp: skippedCacheRecovery,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", {
				name: "The page bundle could not be loaded",
			}),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Clear cached app and reload" }),
		);
		await expect(
			canvas.findByText(
				"The current app cache was left untouched because a complete replacement was not available.",
			),
		).resolves.toBeInTheDocument();
	},
};

export const CacheVersionMismatch: Story = {
	args: {
		error: new Error("asset cache version mismatch"),
		category: "cache-mismatch",
		onReload: fn(),
		onClearCachedApp: skippedCacheRecovery,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByTestId("framework-error-category"),
		).toHaveAttribute("data-error-category", "cache-mismatch");
		await expectReloadAction(canvasElement);
		await expect(
			canvas.getByRole("button", { name: "Clear cached app and reload" }),
		).toBeInTheDocument();
	},
};

export const Offline: Story = {
	args: {
		error: new TypeError("Failed to fetch"),
		category: "offline",
		isOnline: false,
		onReload: fn(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "You are offline" }),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
		await expect(
			canvas.queryByRole("button", { name: "Clear cached app and reload" }),
		).not.toBeInTheDocument();
	},
};

export const ApiIncompatibility: Story = {
	args: {
		error: new BackendApiError({
			status: 409,
			code: "api_incompatible",
			message: "API compatibility window does not include this client.",
		}),
		category: "api-incompatibility",
		onReload: fn(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", {
				name: "The backend does not support this web app",
			}),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
	},
};

export const ReactRuntime: Story = {
	args: {
		error: new Error("Minified React error #185"),
		category: "react-runtime",
		onReload: fn(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "The app hit a runtime error" }),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
	},
};

export const Unknown: Story = {
	args: {
		error: new Error("unexpected runtime failure"),
		category: "unknown",
		onReload: fn(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", {
				name: "The app encountered an unexpected error",
			}),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
	},
};

export const RepeatedFailure: Story = {
	args: {
		error: new Error("recovery failed again"),
		category: "unknown",
		repeatFailure: true,
		onReload: fn(),
		onClearCachedApp: skippedCacheRecovery,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByTestId("repeat-failure")).toHaveTextContent(
			"This happened again. Safe cache recovery is available.",
		);
		await expect(
			canvas.getByRole("button", { name: "Clear cached app and reload" }),
		).toBeInTheDocument();
		await expectReloadAction(canvasElement);
	},
};
