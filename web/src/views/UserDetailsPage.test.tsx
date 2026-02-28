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
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserAccess,
	replaceAdminUserAccess,
} from "../api/adminUserAccess";
import { fetchAdminUserNodeQuotaStatus } from "../api/adminUserNodeQuotaStatus";
import { fetchAdminUser } from "../api/adminUsers";
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
vi.mock("../api/adminUserNodeQuotaStatus");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminUserAccess");

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
	accessEndpointIds?: string[];
	nodes?: Array<{
		node_id: string;
		node_name: string;
		api_base_url: string;
		access_host: string;
		quota_limit_bytes?: number;
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
}) {
	const { userId } = args;
	vi.mocked(fetchAdminUser).mockResolvedValue({
		user_id: userId,
		display_name: "Ivan",
		subscription_token: "subtoken",
		priority_tier: "p3",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});

	const nodes = (
		args.nodes ?? [
			{
				node_id: "n-tokyo",
				node_name: "Tokyo",
				api_base_url: "http://localhost",
				access_host: "localhost",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: 0,
				},
			},
		]
	).map((n) => ({
		...n,
		quota_limit_bytes: n.quota_limit_bytes ?? 0,
	}));
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
		{
			endpoint_id: "ep-b",
			node_id: "n-tokyo",
			tag: "tokyo-ss",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 8443,
			meta: {},
		},
	];
	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: endpoints,
	});

	const accessEndpointIds = args.accessEndpointIds ?? [];
	vi.mocked(fetchAdminUserAccess).mockResolvedValue({
		items: accessEndpointIds.map((endpointId, index) => {
			const endpoint = endpoints.find(
				(item) => item.endpoint_id === endpointId,
			);
			if (!endpoint)
				throw new Error(`missing endpoint in fixture: ${endpointId}`);
			return {
				membership: {
					user_id: userId,
					node_id: endpoint.node_id,
					endpoint_id: endpoint.endpoint_id,
				},
				grant: {
					grant_id: `grant-${index + 1}`,
					enabled: true,
					quota_limit_bytes: 0,
					note: null,
					credentials:
						endpoint.kind === "vless_reality_vision_tcp"
							? {
									vless: {
										uuid: `22222222-2222-2222-2222-${String(index + 1).padStart(12, "0")}`,
										email: `grant:${index + 1}`,
									},
								}
							: {
									ss2022: {
										method: "2022-blake3-aes-128-gcm",
										password: `mock-password-${index + 1}`,
									},
								},
				},
			};
		}),
	});
	vi.mocked(replaceAdminUserAccess).mockResolvedValue({
		items: [],
	});

	vi.mocked(fetchAdminUserNodeQuotaStatus).mockResolvedValue({
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				user_id: userId,
				node_id: nodes[0].node_id,
				quota_limit_bytes: 0,
				used_bytes: 0,
				remaining_bytes: 0,
				cycle_end_at: new Date(
					Date.now() + 10 * 24 * 60 * 60 * 1000,
				).toISOString(),
				quota_reset_source: "user",
			},
		],
	});
}

describe("<UserDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("renders tabs and can switch between User / Access / Quota usage", async () => {
		setupHappyPathMocks({ userId: "u_01HUSERAAAAAA" });
		const view = renderPage();

		expect(
			await within(view.container).findByRole("heading", { name: "Profile" }),
		).toBeInTheDocument();
		expect(
			within(view.container).getByRole("button", { name: "Access" }),
		).toBeInTheDocument();
		expect(
			within(view.container).getByRole("button", { name: "Quota usage" }),
		).toBeInTheDocument();

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Access" }),
		);
		expect(
			await within(view.container).findByRole("heading", { name: "Access" }),
		).toBeInTheDocument();

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Quota usage" }),
		);
		expect(
			await within(view.container).findByRole("heading", {
				name: "Quota usage",
			}),
		).toBeInTheDocument();
	});

	it("applies user access via hard-cut API", async () => {
		setupHappyPathMocks({ userId: "u_01HUSERAAAAAA", accessEndpointIds: [] });
		const view = renderPage();

		fireEvent.click(
			await within(view.container).findByRole("button", { name: "Access" }),
		);
		await within(view.container).findByRole("heading", { name: "Access" });

		fireEvent.click(
			within(view.container).getByLabelText("Toggle Tokyo VLESS"),
		);
		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply changes" }),
		);

		await waitFor(() => {
			expect(replaceAdminUserAccess).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					items: [{ endpoint_id: "ep-a", note: null }],
				},
			);
		});
	});
});
