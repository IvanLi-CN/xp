import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "../Button";
import { Alert } from "./alert";

const meta = {
	title: "UI/Alert",
	component: Alert,
	tags: ["autodocs", "coverage-ui"],
	args: { tone: "info", children: "Cluster data is available." },
	parameters: { layout: "fullscreen" },
} satisfies Meta<typeof Alert>;

export default meta;
type Story = StoryObj<typeof meta>;

export const StatusGallery: Story = {
	render: () => (
		<div
			data-visual-evidence-surface
			className="inline-block bg-background p-6 text-foreground"
		>
			<div
				data-visual-evidence-target
				className="flex w-[min(36rem,calc(100vw-3rem))] flex-col gap-3"
			>
				<Alert tone="info">Cluster data is available.</Alert>
				<Alert tone="success">The change was saved.</Alert>
				<Alert tone="warning" className="flex-col">
					<p>The node needs a reload before it can be checked.</p>
					<div className="flex flex-wrap gap-2">
						<Button size="sm" variant="ghost">
							Cancel
						</Button>
						<Button size="sm">Reload once</Button>
					</div>
				</Alert>
				<Alert tone="error" className="flex-col">
					<p>Two entries could not be saved.</p>
					<ul className="list-inside list-disc">
						<li>Check the node connection.</li>
					</ul>
				</Alert>
			</div>
		</div>
	),
};

export const Mobile: Story = {
	...StatusGallery,
	parameters: {
		viewport: {
			defaultViewport: "alertMobile393",
			viewports: {
				alertMobile393: {
					name: "Alert mobile (393x852)",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
	},
};
