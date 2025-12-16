/** @type {import('tailwindcss').Config} */
import daisyui from "daisyui";

export default {
	content: [
		"./index.html",
		"./src/**/*.{ts,tsx,js,jsx}",
		"./.storybook/**/*.{ts,tsx}",
	],
	theme: {
		extend: {},
	},
	plugins: [daisyui],
	daisyui: {
		themes: ["light", "dark", "cupcake"],
	},
};
