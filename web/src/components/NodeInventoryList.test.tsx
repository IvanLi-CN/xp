import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AdminNodeRuntimeListItem } from "../api/adminNodeRuntime";
import { NodeInventoryList } from "./NodeInventoryList";
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

const baseNodes: AdminNodeRuntimeListItem[] = [
	{
		node_id: "node-1",
		node_name: "tokyo-1",
		api_base_url: "https://node-1.example.com",
		access_host: "node-1.example.com",
		summary: {
			status: "up",
			updated_at: "2026-03-01T00:00:00Z",
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
				slot_start: "2026-03-01T00:00:00Z",
				status: "up",
			},
		],
	},
	{
		node_id: "node-2",
		node_name: "",
		api_base_url: "https://node-2.example.com",
		access_host: "node-2.example.com",
		summary: {
			status: "degraded",
			updated_at: "2026-03-01T00:00:00Z",
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
				slot_start: "2026-03-01T00:00:00Z",
				status: "down",
			},
		],
	},
];

describe("<NodeInventoryList />", () => {
	it("renders icon-only node links without button styles", async () => {
		render(
			<UiPrefsProvider>
				<NodeInventoryList
					items={baseNodes}
					partial={false}
					unreachableNodes={[]}
				/>
			</UiPrefsProvider>,
		);

		const links = await screen.findAllByRole("link", {
			name: /open node panel:/i,
		});
		const uniqueHrefs = new Set(
			links.map((link) => link.getAttribute("href")).filter(Boolean),
		);
		expect(uniqueHrefs).toEqual(new Set(["/nodes/node-1", "/nodes/node-2"]));
		expect(links.some((link) => !/\bbtn\b/.test(link.className))).toBe(true);

		for (const nodeName of screen.getAllByText("tokyo-1")) {
			expect(nodeName.closest("a")).toBeNull();
		}
		expect(screen.getAllByText("(unnamed)").length).toBeGreaterThan(0);
		expect(screen.getByRole("table")).toBeInTheDocument();
		expect(screen.getAllByText("API base URL").length).toBeGreaterThan(0);
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
									updated_at: "2026-03-01T00:00:00Z",
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
});
