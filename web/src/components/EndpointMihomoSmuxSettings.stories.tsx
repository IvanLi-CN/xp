import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import type { MihomoSmuxConfig } from "../api/adminEndpoints";
import { EndpointMihomoSmuxSettings } from "./EndpointMihomoSmuxSettings";

type SettingsStoryProps = {
	available: boolean;
	disabled: boolean;
	enabled: boolean;
};

function SettingsStory({ available, disabled, enabled }: SettingsStoryProps) {
	const [config, setConfig] = useState<MihomoSmuxConfig>({
		enabled,
		max_connections: 4,
		min_streams: 4,
		only_tcp: true,
	});
	const [maxConnections, setMaxConnections] = useState("4");
	const [minStreams, setMinStreams] = useState("4");

	return (
		<div className="w-[420px] max-w-full p-4">
			<EndpointMihomoSmuxSettings
				available={available}
				config={config}
				disabled={disabled}
				inputClass=""
				maxConnections={maxConnections}
				minStreams={minStreams}
				onConfigChange={setConfig}
				onMaxConnectionsChange={setMaxConnections}
				onMinStreamsChange={setMinStreams}
			/>
		</div>
	);
}

const meta = {
	title: "Components/EndpointMihomoSmuxSettings",
	component: SettingsStory,
	tags: ["autodocs", "coverage-ui", "endpoint-mihomo-smux"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"SS2022-only Mihomo SMux settings. VLESS and REALITY endpoints do not render this component.",
			},
		},
	},
	args: {
		available: true,
		disabled: false,
		enabled: true,
	},
} satisfies Meta<typeof SettingsStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByText("高级设置：SS2022 连接复用 (SMux)"),
		);
		await expect(await canvas.findByLabelText("启用 SMux")).toBeChecked();
		await expect(await canvas.findByLabelText("最大物理连接数")).toHaveValue(4);
	},
};

export const Disabled: Story = {
	args: {
		disabled: true,
	},
};

export const UnsupportedServer: Story = {
	args: {
		available: false,
	},
};
