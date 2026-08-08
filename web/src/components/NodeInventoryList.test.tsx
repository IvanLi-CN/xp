import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminNodeRuntimeListItem } from "../api/adminNodeRuntime";
import {
	LIST_LAYOUT_BREAKPOINT_PX,
	NodeInventoryList,
} from "./NodeInventoryList";
import { UiPrefsProvider } from "./UiPrefs";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			params,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
			params?: Record<string, string>;
		}) => {
			let href = to ?? "#";
			if (params) {
				for (const [key, value] of Object.entries(params)) {
					href = href.replace(`$${key}`, value);
				}
			}
			return (
				<a href={href} {...rest}>
					{children}
				</a>
			);
		},
	};
});

vi.mock("./Icon", () => ({
	Icon: ({ ariaLabel }: { ariaLabel?: string }) => (
		<span aria-hidden={ariaLabel ? undefined : "true"} aria-label={ariaLabel} />
	),
}));

vi.mock("./auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("./auth")>();
	return {
		...actual,
		readAdminToken: () => "xp_admin_token",
	};
});

const baseNodes: AdminNodeRuntimeListItem[] = [
	{
		node_id: fixtureCatalog.slotString.s32(),
		node_name: fixtureCatalog.slotString.s33(),
		api_base_url: fixtureCatalog.slotString.s178(),
		access_host: fixtureCatalog.slotString.s179(),
		summary: {
			status: "up",
			updated_at: fixtureCatalog.slotString.s91(),
		},
		components: [
			{
				component: "xp",
				status: "up",
				consecutive_failures: 0,
				recoveries_observed: 2,
				restart_attempts: 0,
			},
		],
		recent_slots: [
			{
				slot_start: fixtureCatalog.slotString.s91(),
				status: "up",
			},
		],
	},
	{
		node_id: fixtureCatalog.slotString.s36(),
		node_name: fixtureCatalog.slotString.s99(),
		api_base_url: fixtureCatalog.slotString.s180(),
		access_host: fixtureCatalog.slotString.s181(),
		summary: {
			status: "degraded",
			updated_at: fixtureCatalog.slotString.s91(),
		},
		components: [
			{
				component: "xray",
				status: "down",
				consecutive_failures: 1,
				recoveries_observed: 0,
				restart_attempts: 1,
			},
		],
		recent_slots: [
			{
				slot_start: fixtureCatalog.slotString.s91(),
				status: "down",
			},
		],
	},
];

describe("<NodeInventoryList />", () => {
	beforeEach(() => {
		window.history.replaceState(
			{},
			fixtureCatalog.slotString.s99(),
			"/nodes?view=table&login_token=old-token#history",
		);
	});

	it("renders a desktop table with details and cross-node actions", async () => {
		render(
			<UiPrefsProvider>
				<NodeInventoryList
					items={baseNodes}
					partial={false}
					unreachableNodes={[]}
				/>
			</UiPrefsProvider>,
		);

		expect(screen.getByRole("table")).toBeInTheDocument();
		expect(
			screen.getByRole("columnheader", { name: "Actions" }),
		).toBeInTheDocument();
		const detailsLinks = await screen.findAllByRole("link", {
			name: "Details",
		});
		expect(detailsLinks.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.slotString.s32()}`,
			`/nodes/${fixtureCatalog.slotString.s36()}`,
		]);
		const openOnNodeLinks = screen.getAllByRole("link", {
			name: "Open on node",
		});
		expect(openOnNodeLinks.map((link) => link.getAttribute("href"))).toEqual([
			`${fixtureCatalog.slotString.s178()}/nodes?view=table&login_token=xp_admin_token#history`,
			`${fixtureCatalog.slotString.s180()}/nodes?view=table&login_token=xp_admin_token#history`,
		]);
		expect(
			screen.getAllByText(fixtureCatalog.slotString.s99()).length,
		).toBeGreaterThan(0);
		expect(screen.queryByText("API base URL")).toBeNull();
		expect(screen.queryByText("Access host")).toBeNull();
		expect(screen.queryByText("Components")).toBeNull();
		expect(screen.queryByText("7d (30m)")).toBeNull();
	});

	it("shows partial runtime warning", () => {
		render(
			<UiPrefsProvider>
				<NodeInventoryList
					items={baseNodes}
					partial
					unreachableNodes={["node-3"]}
				/>
			</UiPrefsProvider>,
		);

		expect(
			screen.getByText(/Partial result: unreachable node\(s\):/i),
		).toBeInTheDocument();
		expect(screen.getByText("node-3")).toBeInTheDocument();
	});

	it("renders problematic badges when ResizeObserver is unavailable", async () => {
		const originalResizeObserver = globalThis.ResizeObserver;
		Object.defineProperty(globalThis, "ResizeObserver", {
			configurable: true,
			writable: true,
			value: undefined,
		});

		try {
			const { container } = render(
				<UiPrefsProvider>
					<NodeInventoryList
						items={[
							{
								...baseNodes[0],
								summary: {
									status: "degraded",
									updated_at: fixtureCatalog.slotString.s91(),
								},
								components: [
									{
										component: "xp",
										status: "down",
										consecutive_failures: 1,
										recoveries_observed: 0,
										restart_attempts: 1,
									},
									{
										component: "xray",
										status: "down",
										consecutive_failures: 1,
										recoveries_observed: 0,
										restart_attempts: 1,
									},
								],
							},
						]}
						partial={false}
						unreachableNodes={[]}
					/>
				</UiPrefsProvider>,
			);

			expect(container.querySelector("[title='xp:down']")).toBeInTheDocument();
			expect(
				container.querySelector("[title='xray:down']"),
			).toBeInTheDocument();
		} finally {
			Object.defineProperty(globalThis, "ResizeObserver", {
				configurable: true,
				writable: true,
				value: originalResizeObserver,
			});
		}
	});

	it("renders mobile cards below the table breakpoint with both actions", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: LIST_LAYOUT_BREAKPOINT_PX - 1,
		});

		try {
			render(
				<UiPrefsProvider>
					<NodeInventoryList
						items={baseNodes}
						partial={false}
						unreachableNodes={[]}
					/>
				</UiPrefsProvider>,
			);

			expect(screen.queryByRole("table")).toBeNull();
			expect(
				await screen.findAllByRole("link", { name: "Details" }),
			).toHaveLength(2);
			expect(
				screen.getAllByRole("link", { name: "Open on node" }),
			).toHaveLength(2);
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});
});
