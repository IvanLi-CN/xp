import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import { useEffect } from "react";

import { SubscriptionPreviewDialog } from "./SubscriptionPreviewDialog";

const meta: Meta<typeof SubscriptionPreviewDialog> = {
	title: "Components/SubscriptionPreviewDialog",
	component: SubscriptionPreviewDialog,
};

export default meta;
type Story = StoryObj<typeof SubscriptionPreviewDialog>;

function DesignBackdrop({
	children,
}: {
	children: React.ReactNode;
}): React.ReactElement {
	useEffect(() => {
		const html = document.documentElement;
		const body = document.body;
		const prevHtmlBg = html.style.background;
		const prevBodyBg = body.style.background;
		const prevBodyMargin = body.style.margin;
		const prevBodyPadding = body.style.padding;

		html.style.background = "#0b1020";
		body.style.background = "#0b1020";
		body.style.margin = "0";
		body.style.padding = "0";

		return () => {
			html.style.background = prevHtmlBg;
			body.style.background = prevBodyBg;
			body.style.margin = prevBodyMargin;
			body.style.padding = prevBodyPadding;
		};
	}, []);

	return (
		<div className="w-screen h-screen" style={{ background: "#0b1020" }}>
			{children}
		</div>
	);
}

// Keep the demo content aligned with the design SVG to enable pixel-level
// comparison in Storybook.
const sampleClash = `proxies:
- name: "alice-alpha-edge-1"
  type: vless
  server: "chatgpt.com"
  port: 443
  flow: xtls-rprx-vision
  servername: "chatgpt.com"
  client-fingerprint: chrome
  reality-opts:
    public-key: TMuezmWCsXxGSHGkRqr9Yyyc9kJpkmipw8gCD6VnPmM
    short-id: b4ddd2affe2585a0
`;

export const Clash: Story = {
	render: () => {
		const [open, setOpen] = useState(true);
		return (
			<DesignBackdrop>
				<SubscriptionPreviewDialog
					open={open}
					onClose={() => setOpen(false)}
					subscriptionUrl="https://example.com/api/sub/sub-demo?format=clash"
					format="clash"
					loading={false}
					content={sampleClash}
				/>
			</DesignBackdrop>
		);
	},
};

export const Raw: Story = {
	render: () => {
		const [open, setOpen] = useState(true);
		return (
			<DesignBackdrop>
				<SubscriptionPreviewDialog
					open={open}
					onClose={() => setOpen(false)}
					subscriptionUrl="https://example.com/api/sub/sub-demo?format=raw"
					format="raw"
					loading={false}
					content={
						"vless://example-host?encryption=none\nvless://second-host?encryption=none\n"
					}
				/>
			</DesignBackdrop>
		);
	},
};
