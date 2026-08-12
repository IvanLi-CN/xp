import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import type { VlessRealityTransport } from "../api/adminEndpoints";
import { cn } from "../lib/utils";
import { EndpointVlessTransportSettings } from "./EndpointVlessTransportSettings";

type SettingsStoryProps = {
	disabled: boolean;
	existing: boolean;
	initialValue: VlessRealityTransport;
	visible: boolean;
};

function SettingsStory({
	disabled,
	existing,
	initialValue,
	visible,
}: SettingsStoryProps) {
	const [value, setValue] = useState(initialValue);

	return (
		<div
			className="mx-auto w-full max-w-[568px] bg-background p-4 sm:p-6"
			data-testid="vless-transport-evidence-frame"
		>
			<div
				className={cn(
					"mx-auto w-full max-w-[520px] border-2 border-[#94bfc4] bg-[#cfe9ec] p-8",
					"dark:border-[#28565a] dark:bg-[#172728]",
				)}
			>
				<EndpointVlessTransportSettings
					changed={existing && value !== initialValue}
					disabled={disabled}
					existing={existing}
					onValueChange={setValue}
					value={value}
					visible={visible}
				/>
			</div>
		</div>
	);
}

const meta = {
	title: "Components/EndpointVlessTransportSettings",
	component: SettingsStory,
	tags: ["autodocs", "coverage-ui", "endpoint-vless-xhttp"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component: [
					"VLESS Reality transport mode.",
					"XHTTP/XMUX reuses one HTTP/2 transport after warm-up;",
					"Vision TCP remains the legacy compatibility mode.",
				].join(" "),
			},
		},
	},
	args: {
		disabled: false,
		existing: false,
		initialValue: "xhttp",
		visible: true,
	},
} satisfies Meta<typeof SettingsStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const DefaultXhttp: Story = {
	parameters: {
		viewport: {
			defaultViewport: "vlessTransportDesktop",
			viewports: {
				vlessTransportDesktop: {
					name: "VLESS transport desktop (1280x900)",
					styles: { height: "900px", width: "1280px" },
					type: "desktop",
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByText("Advanced: VLESS transport"));
		await expect(
			await canvas.findByRole("radio", { name: "XHTTP / XMUX" }),
		).toBeChecked();
		await userEvent.hover(
			await canvas.findByRole("button", { name: "About VLESS transport" }),
		);
		await expect(
			await within(document.body).findByText(
				/reusable HTTP\/2 transport connections/,
			),
		).toBeInTheDocument();
	},
};

export const ExistingVisionTcp: Story = {
	args: {
		existing: true,
		initialValue: "vision_tcp",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByText("Advanced: VLESS transport"));
		await userEvent.click(
			await canvas.findByRole("radio", { name: "XHTTP / XMUX" }),
		);
		await userEvent.hover(
			await canvas.findByRole("button", { name: "Transport change impact" }),
		);
		await expect(
			await within(document.body).findByText(
				/Changing this mode rebuilds the inbound/,
			),
		).toBeInTheDocument();
	},
};

export const ExistingVisionTcpMobile: Story = {
	...ExistingVisionTcp,
	parameters: {
		viewport: {
			defaultViewport: "vlessTransportMobile",
			viewports: {
				vlessTransportMobile: {
					name: "VLESS transport mobile (393x852)",
					styles: { height: "852px", width: "393px" },
					type: "mobile",
				},
			},
		},
	},
};

export const Disabled: Story = {
	args: {
		disabled: true,
	},
};

export const UnsupportedServer: Story = {
	args: {
		visible: false,
	},
};
