import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import { DemoInspectorControls } from "./DemoInspector";
import { DEFAULT_DEMO_CONTROLS } from "./shareState";

const meta = {
	title: "Demo/Inspector",
	component: DemoInspectorControls,
	parameters: {
		layout: "centered",
	},
	args: {
		controls: DEFAULT_DEMO_CONTROLS,
		operations: [],
		shareUrl:
			"https://xp.example/demo/users/user-fixture-c?demo=normal&d_profile=valid",
		onChange: () => undefined,
		onReset: () => undefined,
		onClose: () => undefined,
		onWalkthrough: () => undefined,
	},
} satisfies Meta<typeof DemoInspectorControls>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const MissingProfile: Story = {
	args: {
		controls: { ...DEFAULT_DEMO_CONTROLS, profile: "missing" },
		operations: [
			{
				id: 1,
				method: "PUT",
				route: "/api/admin/users/:id/subscription-mihomo-profile",
				status: 200,
			},
		],
	},
};

export const AnonymousIncident: Story = {
	args: {
		controls: {
			...DEFAULT_DEMO_CONTROLS,
			scene: "incident",
			persona: "anonymous",
			network: "error",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.selectOptions(canvas.getByLabelText("Network"), "offline");
		await expect(canvas.getByLabelText("Network")).toHaveValue("offline");
	},
};

export const LargeTenantOperations: Story = {
	args: {
		controls: { ...DEFAULT_DEMO_CONTROLS, scene: "large" },
		operations: Array.from({ length: 4 }, (_, index) => ({
			id: index + 1,
			method: "POST",
			route: `/api/demo/action/${index + 1}`,
			status: 200,
		})),
	},
};
