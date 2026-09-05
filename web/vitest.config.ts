import path from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
	plugins: [react()],
	resolve: {
		alias: {
			"@": path.resolve(__dirname, "./src"),
			"virtual:pwa-register/react": path.resolve(
				__dirname,
				"./src/testFixtures/pwaRegister.ts",
			),
		},
	},
	test: {
		environment: "jsdom",
		setupFiles: ["./src/setupTests.ts"],
		include: ["src/**/*.test.{ts,tsx}"],
		exclude: ["tests/e2e/**"],
		testTimeout: 15_000,
	},
});
