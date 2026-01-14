/** @type {import('tailwindcss').Config} */
import daisyui from "daisyui";
import daisyuiTheme from "daisyui/theme";
import themesObject from "daisyui/theme/object";

export default {
	content: [
		"./index.html",
		"./src/**/*.{ts,tsx,js,jsx}",
		"./.storybook/**/*.{ts,tsx}",
	],
	theme: {
		extend: {},
	},
	plugins: [
		daisyui,
		daisyuiTheme({
			name: "xp-light",
			default: true,
			"color-scheme": "light",
			...themesObject.winter,
			"--color-primary": "oklch(63% 0.22 205)", // tropical teal
			"--color-primary-content": "oklch(98% 0.01 205)",
			"--color-secondary": "oklch(70% 0.21 45)", // warm sand
			"--color-secondary-content": "oklch(18% 0.02 45)",
			"--color-accent": "oklch(72% 0.20 150)", // mint
			"--color-accent-content": "oklch(18% 0.02 150)",
			"--radius-box": "0.75rem",
			"--radius-field": "1.25rem",
			"--depth": "0",
			"--noise": "0",
		}),
		daisyuiTheme({
			name: "xp-dark",
			prefersdark: true,
			"color-scheme": "dark",
			...themesObject.dim,
			"--color-primary": "oklch(70% 0.18 205)", // tropical teal
			"--color-primary-content": "oklch(16% 0.02 205)",
			"--color-secondary": "oklch(72% 0.16 45)",
			"--color-secondary-content": "oklch(16% 0.02 45)",
			"--color-accent": "oklch(74% 0.16 150)",
			"--color-accent-content": "oklch(16% 0.02 150)",
			"--radius-box": "0.75rem",
			"--radius-field": "1.25rem",
			"--depth": "0",
			"--noise": "0",
		}),
	],
	daisyui: {
		themes: [],
	},
};
