import * as path from "node:path";

import type { StorybookConfig } from "@storybook/react-vite";
import tailwindcss from "@tailwindcss/vite";
import { mergeConfig } from "vite";

const config: StorybookConfig = {
	stories: ["../src/**/*.stories.@(ts|tsx)", "../src/**/*.mdx"],
	addons: ["@storybook/addon-essentials", "@storybook/addon-interactions"],
	framework: {
		name: "@storybook/react-vite",
		options: {},
	},
	docs: {
		autodocs: "tag",
	},
	async viteFinal(baseConfig) {
		return mergeConfig(baseConfig, {
			plugins: [tailwindcss()],
			resolve: {
				alias: {
					"@": path.resolve(__dirname, "../src"),
				},
			},
		});
	},
};

export default config;
