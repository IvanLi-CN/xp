import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
	fetchAdminClusterSettings,
	putAdminClusterSettings,
} from "../api/adminClusterSettings";
import { ToastProvider } from "../components/Toast";
import { createQueryClient } from "../queryClient";
import { ClusterSettingsPage } from "./ClusterSettingsPage";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
		}) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
	};
});

vi.mock("../api/adminClusterSettings");

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: () => "admintoken",
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<ToastProvider>
				<ClusterSettingsPage />
			</ToastProvider>
		</QueryClientProvider>,
	);
}

describe("<ClusterSettingsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		vi.mocked(fetchAdminClusterSettings).mockResolvedValue({
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			legacy_fallback_in_use: true,
		});
		vi.mocked(putAdminClusterSettings).mockResolvedValue({
			ip_geo_enabled: true,
			ip_geo_origin: "https://geo.example.test",
			legacy_fallback_in_use: false,
		});
	});

	it("loads, edits, and saves cluster ip geo settings", async () => {
		renderPage();

		expect(
			await screen.findByText(
				"Current value comes from the leader's legacy env fallback.",
			),
		).toBeInTheDocument();

		const toggle = screen.getByRole("checkbox", {
			name: "Enable IP geo enrichment",
		});
		fireEvent.click(toggle);

		const originInput = screen.getByLabelText("country.is origin");
		fireEvent.change(originInput, {
			target: { value: "https://geo.example.test/" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(putAdminClusterSettings).toHaveBeenCalledWith("admintoken", {
				ip_geo_enabled: true,
				ip_geo_origin: "https://geo.example.test/",
			});
		});

		expect(
			await screen.findByDisplayValue("https://geo.example.test"),
		).toBeDefined();
		expect(
			await screen.findByText("Cluster state is active."),
		).toBeInTheDocument();
	});
});
