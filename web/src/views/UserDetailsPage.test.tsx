import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserGrants,
	putAdminUserGrants,
} from "../api/adminUserGrants";
import { fetchAdminUserNodeQuotaStatus } from "../api/adminUserNodeQuotaStatus";
import {
	fetchAdminUserNodeQuotas,
	putAdminUserNodeQuota,
} from "../api/adminUserNodeQuotas";
import {
	deleteAdminUser,
	fetchAdminUser,
	patchAdminUser,
	resetAdminUserToken,
} from "../api/adminUsers";
import { fetchSubscription } from "../api/subscription";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { UserDetailsPage } from "./UserDetailsPage";

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
		useNavigate: () => vi.fn(),
		useParams: () => ({ userId: "u_01HUSERAAAAAA" }),
	};
});

vi.mock("../api/adminUsers");
vi.mock("../api/adminNodes");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminUserGrants");
vi.mock("../api/adminUserNodeQuotas");
vi.mock("../api/adminUserNodeQuotaStatus");
vi.mock("../api/subscription");

const { mockReadAdminToken } = vi.hoisted(() => ({
	mockReadAdminToken: vi.fn(() => "admintoken"),
}));

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
			<UiPrefsProvider>
				<ToastProvider>
					<UserDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

function setupMocks(args?: {
	grants?: Array<{
		grant_id: string;
		user_id: string;
		endpoint_id: string;
		enabled: boolean;
		quota_limit_bytes: number;
		note?: string | null;
	}>;
}) {
	vi.mocked(fetchAdminUser).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		display_name: "Ivan",
		subscription_token: "subtoken",
		priority_tier: "p2",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});

	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: "node-tokyo",
				node_name: "Tokyo",
				api_base_url: "https://tokyo.example.com",
				access_host: "tokyo.example.com",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
		],
	});

	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: [
			{
				endpoint_id: "ep-vless",
				node_id: "node-tokyo",
				tag: "tokyo-vless",
				kind: "vless_reality_vision_tcp",
				port: 443,
				meta: {},
			},
			{
				endpoint_id: "ep-ss",
				node_id: "node-tokyo",
				tag: "tokyo-ss",
				kind: "ss2022_2022_blake3_aes_128_gcm",
				port: 8443,
				meta: {},
			},
		],
	});

	vi.mocked(fetchAdminUserGrants).mockResolvedValue({
		items:
			args?.grants?.map((grant) => ({
				...grant,
				note: grant.note ?? null,
				credentials: {
					vless:
						grant.endpoint_id === "ep-vless"
							? {
									uuid: "00000000-0000-0000-0000-000000000001",
									email: `grant:${grant.grant_id}`,
								}
							: undefined,
					ss2022:
						grant.endpoint_id === "ep-ss"
							? {
									method: "2022-blake3-aes-128-gcm",
									password: "server:user",
								}
							: undefined,
				},
			})) ?? [],
	});

	vi.mocked(fetchAdminUserNodeQuotas).mockResolvedValue({
		items: [
			{
				user_id: "u_01HUSERAAAAAA",
				node_id: "node-tokyo",
				quota_limit_bytes: 0,
				quota_reset_source: "user",
			},
		],
	});

	vi.mocked(fetchAdminUserNodeQuotaStatus).mockResolvedValue({
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				user_id: "u_01HUSERAAAAAA",
				node_id: "node-tokyo",
				quota_limit_bytes: 1024,
				used_bytes: 0,
				remaining_bytes: 1024,
				cycle_end_at: null,
				quota_reset_source: "user",
			},
		],
	});

	vi.mocked(putAdminUserGrants).mockResolvedValue({
		created: 0,
		updated: 0,
		deleted: 0,
		items: [],
	});
	vi.mocked(putAdminUserNodeQuota).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		node_id: "node-tokyo",
		quota_limit_bytes: 0,
		quota_reset_source: "user",
	});
	vi.mocked(patchAdminUser).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		display_name: "Ivan",
		subscription_token: "subtoken",
		priority_tier: "p2",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});
	vi.mocked(deleteAdminUser).mockResolvedValue(undefined);
	vi.mocked(resetAdminUserToken).mockResolvedValue({
		subscription_token: "sub_new",
	});
	vi.mocked(fetchSubscription).mockResolvedValue(
		"vless://example-host?encryption=none",
	);
}

describe("<UserDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		mockReadAdminToken.mockReturnValue("admintoken");
	});

	afterEach(() => {
		cleanup();
	});

	it("initializes access matrix from existing user grants", async () => {
		setupMocks({
			grants: [
				{
					grant_id: "grant-1",
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-vless",
					enabled: true,
					quota_limit_bytes: 0,
				},
			],
		});
		renderPage();

		await waitFor(() => {
			expect(fetchAdminUserGrants).toHaveBeenCalled();
		});

		const accessTab = await screenByRole("button", "Access");
		fireEvent.click(accessTab);

		const checkbox = await screenByLabel("Toggle Tokyo VLESS");
		expect((checkbox as HTMLInputElement).checked).toBe(true);
	});

	it("applies selected endpoints via putAdminUserGrants", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(await screenByLabel("Toggle Tokyo VLESS"));
		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserGrants).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					items: [
						{
							endpoint_id: "ep-vless",
							enabled: true,
							quota_limit_bytes: 0,
							note: null,
						},
					],
				},
			);
		});
	});

	it("submits empty items when all access is cleared", async () => {
		setupMocks({
			grants: [
				{
					grant_id: "grant-1",
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-vless",
					enabled: true,
					quota_limit_bytes: 0,
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(await screenByLabel("Toggle Tokyo VLESS"));
		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserGrants).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{ items: [] },
			);
		});
	});

	it("saves edited user profile", async () => {
		setupMocks();
		renderPage();

		const input = await screenByLabel("Display name");
		fireEvent.change(input, { target: { value: "Ivan Updated" } });
		fireEvent.click(await screenByRole("button", "Save user"));

		await waitFor(() => {
			expect(patchAdminUser).toHaveBeenCalled();
		});
	});

	it("loads subscription preview from user tab", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Fetch"));

		await waitFor(() => {
			expect(fetchSubscription).toHaveBeenCalledWith("subtoken", "raw");
		});
		expect(await screenByText("Subscription preview")).toBeTruthy();
	});

	it("shows node quota loading error instead of defaulting to zero", async () => {
		setupMocks();
		vi.mocked(fetchAdminUserNodeQuotas).mockRejectedValueOnce(
			new Error("node quotas unavailable"),
		);
		renderPage();

		expect(await screenByText("Failed to load node quotas")).toBeTruthy();
		expect(await screenByText("node quotas unavailable")).toBeTruthy();
	});

	it("shows access data error with retry action", async () => {
		setupMocks();
		vi.mocked(fetchAdminEndpoints).mockRejectedValue(
			new Error("endpoint load failed"),
		);
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		expect(await screenByText("Failed to load access matrix")).toBeTruthy();
		const retryButton = await screenByRole("button", "Retry access data");
		fireEvent.click(retryButton);

		await waitFor(() => {
			expect(fetchAdminEndpoints).toHaveBeenCalledTimes(2);
		});
	});

	it("shows token-required state when admin token is missing", async () => {
		mockReadAdminToken.mockReturnValue("");
		setupMocks();
		renderPage();

		expect(await screenByText("Admin token required")).toBeTruthy();
		expect(fetchAdminUser).not.toHaveBeenCalled();
	});
});

async function screenByRole(role: string, name: string): Promise<HTMLElement> {
	const { findByRole } = await import("@testing-library/react");
	return findByRole(document.body, role as never, { name });
}

async function screenByLabel(label: string): Promise<HTMLElement> {
	const { findByLabelText } = await import("@testing-library/react");
	return findByLabelText(document.body, label);
}

async function screenByText(text: string): Promise<HTMLElement> {
	const { findByText } = await import("@testing-library/react");
	return findByText(document.body, text);
}
