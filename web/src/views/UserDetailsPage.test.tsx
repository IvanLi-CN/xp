import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	waitFor,
	within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	createAdminGrantGroup,
	deleteAdminGrantGroup,
	fetchAdminGrantGroup,
	fetchAdminGrantGroups,
	replaceAdminGrantGroup,
} from "../api/adminGrantGroups";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserNodeQuotas,
	putAdminUserNodeQuota,
} from "../api/adminUserNodeQuotas";
import { fetchAdminUser } from "../api/adminUsers";
import { BackendApiError } from "../api/backendError";
import type { NodeQuotaReset } from "../api/quotaReset";
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
vi.mock("../api/adminUserNodeQuotas");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminGrantGroups");

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
			<UiPrefsProvider>
				<ToastProvider>
					<UserDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

function setupHappyPathMocks(args: {
	userId: string;
	nodes?: Array<{
		node_id: string;
		node_name: string;
		api_base_url: string;
		access_host: string;
		quota_reset: NodeQuotaReset;
	}>;
	endpoints?: Array<{
		endpoint_id: string;
		node_id: string;
		tag: string;
		kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
		port: number;
		meta: Record<string, unknown>;
	}>;
	nodeQuotas?: Array<{ node_id: string; quota_limit_bytes: number }>;
}) {
	const { userId } = args;
	vi.mocked(fetchAdminUser).mockResolvedValue({
		user_id: userId,
		display_name: "Ivan",
		subscription_token: "subtoken",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});

	const nodes = args.nodes ?? [
		{
			node_id: "n-tokyo",
			node_name: "Tokyo",
			api_base_url: "http://localhost",
			access_host: "localhost",
			quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 0 },
		},
	];
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: nodes,
	});

	const endpoints = args.endpoints ?? [
		{
			endpoint_id: "ep-a",
			node_id: "n-tokyo",
			tag: "tokyo-vless",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {},
		},
	];
	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: endpoints,
	});

	const nodeQuotas = args.nodeQuotas ?? [
		{ node_id: "n-tokyo", quota_limit_bytes: 0 },
	];
	vi.mocked(fetchAdminUserNodeQuotas).mockResolvedValue({
		items: nodeQuotas.map((q) => ({
			user_id: userId,
			node_id: q.node_id,
			quota_limit_bytes: q.quota_limit_bytes,
			quota_reset_source: "user",
		})),
	});
}

describe("<UserDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("renders two tabs and switches between User and Node quotas", async () => {
		setupHappyPathMocks({ userId: "u_01HUSERAAAAAA" });
		vi.mocked(fetchAdminGrantGroup).mockRejectedValue(
			new BackendApiError({ status: 404, message: "not found" }),
		);

		const view = renderPage();

		expect(
			await within(view.container).findByRole("heading", { name: "Profile" }),
		).toBeInTheDocument();
		expect(
			within(view.container).getByRole("button", { name: "User" }),
		).toBeInTheDocument();
		expect(
			within(view.container).getByRole("button", { name: "Node quotas" }),
		).toBeInTheDocument();

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Node quotas" }),
		);
		expect(
			await within(view.container).findByRole("heading", {
				name: "Node quotas",
			}),
		).toBeInTheDocument();

		fireEvent.click(
			within(view.container).getByRole("button", { name: "User" }),
		);
		expect(
			await within(view.container).findByRole("heading", { name: "Profile" }),
		).toBeInTheDocument();
	});

	it("deletes per-user managed group on empty selection (hard cut)", async () => {
		setupHappyPathMocks({
			userId: "u_01HUSERAAAAAA",
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n-tokyo",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
		});
		vi.mocked(fetchAdminGrantGroup).mockRejectedValue(
			new BackendApiError({ status: 404, message: "not found" }),
		);
		vi.mocked(fetchAdminGrantGroups).mockResolvedValue({ items: [] });
		vi.mocked(deleteAdminGrantGroup).mockResolvedValue({ deleted: 0 });

		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", {
				name: "Node quotas",
			}),
		);
		await within(view.container).findByText("Matrix");

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply changes" }),
		);

		await waitFor(() => {
			expect(deleteAdminGrantGroup).toHaveBeenCalledWith(
				"admintoken",
				"managed-u_01huseraaaaaa",
			);
		});
		expect(createAdminGrantGroup).not.toHaveBeenCalled();
		expect(replaceAdminGrantGroup).not.toHaveBeenCalled();
	});

	it("creates per-user managed group when selection is non-empty", async () => {
		setupHappyPathMocks({
			userId: "u_01HUSERAAAAAA",
			nodes: [
				{
					node_id: "n-tokyo",
					node_name: "Tokyo",
					api_base_url: "http://localhost",
					access_host: "localhost",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 0,
					},
				},
			],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n-tokyo",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
			nodeQuotas: [{ node_id: "n-tokyo", quota_limit_bytes: 123 }],
		});
		vi.mocked(fetchAdminGrantGroup).mockRejectedValue(
			new BackendApiError({ status: 404, message: "not found" }),
		);
		vi.mocked(fetchAdminGrantGroups).mockResolvedValue({ items: [] });
		vi.mocked(createAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			members: [],
		});

		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", {
				name: "Node quotas",
			}),
		);
		const cellToggle = await within(view.container).findByLabelText(
			"Toggle Tokyo VLESS",
		);
		fireEvent.click(cellToggle);

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply changes" }),
		);

		await waitFor(() => {
			expect(createAdminGrantGroup).toHaveBeenCalled();
		});
		expect(replaceAdminGrantGroup).not.toHaveBeenCalled();

		const [, payload] = vi.mocked(createAdminGrantGroup).mock.calls[0] ?? [];
		expect(payload).toMatchObject({
			group_name: "managed-u_01huseraaaaaa",
			members: [
				{
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-a",
					enabled: true,
					quota_limit_bytes: 123,
					note: null,
				},
			],
		});
	});

	it("purges user grants from other groups before creating managed group", async () => {
		setupHappyPathMocks({
			userId: "u_01HUSERAAAAAA",
			nodes: [
				{
					node_id: "n-tokyo",
					node_name: "Tokyo",
					api_base_url: "http://localhost",
					access_host: "localhost",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 0,
					},
				},
			],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n-tokyo",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
			nodeQuotas: [{ node_id: "n-tokyo", quota_limit_bytes: 123 }],
		});

		vi.mocked(fetchAdminGrantGroups).mockResolvedValue({
			items: [{ group_name: "legacy-group", member_count: 1 }],
		});
		vi.mocked(fetchAdminGrantGroup).mockImplementation(
			async (_token, groupName) => {
				if (groupName === "managed-u_01huseraaaaaa") {
					throw new BackendApiError({ status: 404, message: "not found" });
				}
				if (groupName === "legacy-group") {
					return {
						group: { group_name: "legacy-group" },
						members: [
							{
								user_id: "u_01HUSERAAAAAA",
								endpoint_id: "ep-a",
								enabled: true,
								quota_limit_bytes: 0,
								note: null,
								credentials: {
									vless: {
										uuid: "00000000-0000-0000-0000-000000000000",
										email: "",
									},
								},
							},
						],
					};
				}
				throw new BackendApiError({ status: 404, message: "not found" });
			},
		);
		vi.mocked(deleteAdminGrantGroup).mockResolvedValue({ deleted: 1 });
		vi.mocked(createAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			members: [],
		});

		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", {
				name: "Node quotas",
			}),
		);
		const cellToggle = await within(view.container).findByLabelText(
			"Toggle Tokyo VLESS",
		);
		fireEvent.click(cellToggle);

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply changes" }),
		);

		await waitFor(() => {
			expect(deleteAdminGrantGroup).toHaveBeenCalledWith(
				"admintoken",
				"legacy-group",
			);
		});
		await waitFor(() => {
			expect(createAdminGrantGroup).toHaveBeenCalled();
		});
	});

	it("replaces per-user managed group when it already exists", async () => {
		setupHappyPathMocks({
			userId: "u_01HUSERAAAAAA",
			nodes: [
				{
					node_id: "n-tokyo",
					node_name: "Tokyo",
					api_base_url: "http://localhost",
					access_host: "localhost",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 0,
					},
				},
			],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n-tokyo",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
			nodeQuotas: [{ node_id: "n-tokyo", quota_limit_bytes: 50 }],
		});
		vi.mocked(fetchAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			members: [
				{
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-a",
					enabled: true,
					quota_limit_bytes: 0,
					note: null,
					credentials: {
						vless: { uuid: "00000000-0000-0000-0000-000000000000", email: "" },
					},
				},
			],
		});
		vi.mocked(fetchAdminGrantGroups).mockResolvedValue({ items: [] });
		vi.mocked(replaceAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			created: 0,
			updated: 1,
			deleted: 0,
		});

		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", {
				name: "Node quotas",
			}),
		);
		await within(view.container).findByText(/Selected 1 \/ 1/);

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply changes" }),
		);

		await waitFor(() => {
			expect(replaceAdminGrantGroup).toHaveBeenCalled();
		});
		expect(createAdminGrantGroup).not.toHaveBeenCalled();
	});

	it("updates node quota and syncs managed group members", async () => {
		setupHappyPathMocks({
			userId: "u_01HUSERAAAAAA",
			nodes: [
				{
					node_id: "n-tokyo",
					node_name: "Tokyo",
					api_base_url: "http://localhost",
					access_host: "localhost",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 0,
					},
				},
			],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n-tokyo",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
			nodeQuotas: [{ node_id: "n-tokyo", quota_limit_bytes: 0 }],
		});

		vi.mocked(fetchAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			members: [
				{
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-a",
					enabled: true,
					quota_limit_bytes: 0,
					note: null,
					credentials: {
						vless: { uuid: "00000000-0000-0000-0000-000000000000", email: "" },
					},
				},
			],
		});
		vi.mocked(fetchAdminGrantGroups).mockResolvedValue({ items: [] });
		vi.mocked(putAdminUserNodeQuota).mockResolvedValue({
			user_id: "u_01HUSERAAAAAA",
			node_id: "n-tokyo",
			quota_limit_bytes: 1024 * 1024 * 1024,
			quota_reset_source: "user",
		});
		vi.mocked(replaceAdminGrantGroup).mockResolvedValue({
			group: { group_name: "managed-u_01huseraaaaaa" },
			created: 0,
			updated: 1,
			deleted: 0,
		});

		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", {
				name: "Node quotas",
			}),
		);
		await within(view.container).findByText("Matrix");

		fireEvent.click(
			within(view.container).getByRole("button", { name: /Quota:/ }),
		);

		const dialog = await within(document.body).findByRole("dialog", {
			name: "Edit node quota",
		});
		fireEvent.change(within(dialog).getByRole("textbox"), {
			target: { value: "1GiB" },
		});
		fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));

		await waitFor(() => {
			expect(putAdminUserNodeQuota).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				"n-tokyo",
				1024 * 1024 * 1024,
				"user",
			);
		});
		await waitFor(() => {
			expect(replaceAdminGrantGroup).toHaveBeenCalledWith(
				"admintoken",
				"managed-u_01huseraaaaaa",
				expect.objectContaining({
					members: [
						expect.objectContaining({
							user_id: "u_01HUSERAAAAAA",
							endpoint_id: "ep-a",
							enabled: true,
							quota_limit_bytes: 1024 * 1024 * 1024,
						}),
					],
				}),
			);
		});
	});
});
