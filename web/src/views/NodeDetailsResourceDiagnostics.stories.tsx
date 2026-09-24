import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import baseMeta, { node } from "./NodeDetailsPage.stories";

const meta = {
	...baseMeta,
	title: "Pages/NodeDetailsPage",
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

async function selectNodeDetailsTab(canvasElement: HTMLElement, label: string) {
	const canvas = within(canvasElement);
	const tab = Array.from(
		canvasElement.querySelectorAll<HTMLElement>('[role="tab"]'),
	).find((element) => element.textContent?.trim() === label);
	if (tab) {
		await userEvent.click(tab);
		return;
	}
	const mobileSelect = canvasElement.querySelector<HTMLElement>(
		'[role="combobox"][aria-label="Node details section"]',
	);
	if (!mobileSelect)
		throw new Error("Node details navigation control is missing");
	await userEvent.click(mobileSelect);
	await userEvent.click(
		await within(canvasElement.ownerDocument.body).findByRole("option", {
			name: label,
		}),
	);
	await expect(
		await canvas.findByText(label, { exact: true }),
	).toBeInTheDocument();
}

export const ResourcesCircuitOpen: Story = {
	tags: ["resource-monitoring"],
	parameters: {
		viewport: {
			defaultViewport: "resourceMobile",
			viewports: {
				resourceMobile: {
					name: "Resource mobile (393x852)",
					styles: { width: "393px", height: "852px" },
					type: "mobile",
				},
			},
		},
		mockApi: {
			data: {
				resourceMonitoringErrors: {
					[node.node_id]: {
						snapshot: {
							status: 503,
							code: "peer_circuit_open",
							message: "resource request is cooling down",
							details: {
								failure_layer: "circuit_breaker",
								cause: "circuit_open",
								confidence: "confirmed",
								target_node_id: node.node_id,
								attempted_path: "direct",
								dispatch_state: "not_dispatched",
								retryable: true,
								retry_after_seconds: 18,
								support_id: "01JRESOURCECIRCUIT",
							},
							headers: { "Retry-After": "18" },
						},
					},
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await selectNodeDetailsTab(canvasElement, "Resources");
		await expect(
			await canvas.findByText("Peer path is cooling down"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: /Retry resource read/i }),
		).toBeDisabled();
	},
};

export const ResourcesHistoryError: Story = {
	tags: ["resource-monitoring"],
	parameters: {
		mockApi: {
			data: {
				resourceMonitoringErrors: {
					[node.node_id]: {
						history: {
							cpu_busy_percent: {
								status: 504,
								code: "peer_transport_timeout",
								message: "history response timed out",
								details: {
									failure_layer: "peer_transport",
									cause: "peer_timeout",
									confidence: "confirmed",
									dispatch_state: "dispatched_no_verified_response",
									retryable: true,
								},
							},
						},
					},
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await selectNodeDetailsTab(canvasElement, "Resources");
		await expect(
			await canvas.findByText("History unavailable"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: "Retry history" }),
		).toBeInTheDocument();
	},
};
