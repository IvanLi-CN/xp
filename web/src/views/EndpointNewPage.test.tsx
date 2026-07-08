import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { ToastProvider } from "../components/Toast";
import { createQueryClient } from "../queryClient";
import { EndpointNewPage } from "./EndpointNewPage";

const { mockNavigate, mockReadAdminToken } = vi.hoisted(() => ({
	mockNavigate: vi.fn(),
	mockReadAdminToken: vi.fn(() => "admintoken"),
}));

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
		useNavigate: () => mockNavigate,
	};
});

vi.mock("../api/adminEndpoints");
vi.mock("../api/adminNodes");

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: mockReadAdminToken,
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<ToastProvider>
				<EndpointNewPage />
			</ToastProvider>
		</QueryClientProvider>,
	);
}

function setupNodeMocks() {
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: "node-alpha",
				node_name: "alpha",
				access_host: "node-xp.example.test",
				api_base_url: "https://node-xp.example.test:443",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
		],
	});
}

describe("EndpointNewPage", () => {
	beforeEach(() => {
		mockNavigate.mockReset();
		mockReadAdminToken.mockReset();
		mockReadAdminToken.mockReturnValue("admintoken");
		vi.clearAllMocks();
		setupNodeMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("submits managed VLESS create payload without legacy reality fields", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: "ep-managed",
			node_id: "node-alpha",
			tag: "ep-managed",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {
				managed_default: true,
			},
		});

		renderPage();

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: " http://127.0.0.1:8080 " },
		});
		fireEvent.change(await screen.findByPlaceholderText("edge.example.com"), {
			target: { value: "Edge.Example.com." },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Add" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: "vless_reality_vision_tcp",
				node_id: "node-alpha",
				port: 443,
				canary_upstream: {
					url: "http://127.0.0.1:8080",
					mode: "auto",
				},
				accepted_authorities: ["edge.example.com:443"],
			});
		});
		expect(createAdminEndpoint).not.toHaveBeenCalledWith(
			"admintoken",
			expect.objectContaining({
				reality: expect.anything(),
			}),
		);
		await waitFor(() => {
			expect(mockNavigate).toHaveBeenCalledWith({
				to: "/endpoints/$endpointId",
				params: { endpointId: "ep-managed" },
			});
		});
	});

	it("shows the backend error when managed create fails", async () => {
		vi.mocked(createAdminEndpoint).mockRejectedValue(
			new Error("400 invalid_request: invalid canary upstream"),
		);

		renderPage();

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: "http://127.0.0.1:8080" },
		});
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		expect(
			await screen.findByText("400 invalid_request: invalid canary upstream"),
		).toBeInTheDocument();
		expect(createAdminEndpoint).toHaveBeenCalledTimes(1);
	});

	it("hides managed VLESS-only fields when switched to SS2022", async () => {
		renderPage();

		expect(
			await screen.findByLabelText("canaryUpstreamUrl"),
		).toBeInTheDocument();
		expect(
			await screen.findByLabelText("accepted host[:port]"),
		).toBeInTheDocument();

		fireEvent.click(await screen.findByLabelText("Kind"));
		fireEvent.click(
			await screen.findByRole("option", { name: "SS2022 BLAKE3 AES-128-GCM" }),
		);

		await waitFor(() => {
			expect(screen.queryByLabelText("canaryUpstreamUrl")).toBeNull();
			expect(screen.queryByLabelText("accepted host[:port]")).toBeNull();
		});
	});
});
