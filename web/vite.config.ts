import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, process.cwd(), "VITE_");
	const backend = env.VITE_BACKEND_PROXY ?? "http://127.0.0.1:62416";

	return {
		plugins: [react()],
		server: {
			host: "127.0.0.1",
			port: 60080,
			strictPort: true,
			proxy: {
				"/api": { target: backend, changeOrigin: true },
				"/events": { target: backend, changeOrigin: true },
			},
		},
	};
});
