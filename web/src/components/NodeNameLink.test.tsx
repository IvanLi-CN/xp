import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
	NodeNameLink,
	compareNodeIdsByDisplayName,
	nodeDisplayName,
} from "./NodeNameLink";

vi.mock("@tanstack/react-router", () => ({
	Link: ({
		children,
		params,
		...rest
	}: {
		children: React.ReactNode;
		params: { nodeId: string };
	}) => (
		<a href={`/nodes/${params.nodeId}`} {...rest}>
			{children}
		</a>
	),
}));

describe("NodeNameLink", () => {
	it("links a resolved name while keeping the ID in accessible metadata", () => {
		render(<NodeNameLink nodeId="node-1" nodeName=" Tokyo edge " />);

		const link = screen.getByRole("link", {
			name: "Open node details: Tokyo edge (node-1)",
		});
		expect(link).toHaveAttribute("href", "/nodes/node-1");
		expect(link).toHaveAttribute("title", "node-1");
		expect(link).toHaveTextContent("Tokyo edge");
	});

	it("shows an unresolved node ID without a link", () => {
		render(<NodeNameLink nodeId="node-1" nodeName="  " />);

		expect(screen.queryByRole("link")).toBeNull();
		expect(screen.getByText("node-1")).toBeInTheDocument();
	});

	it("sorts by display name and uses the ID as a stable tie-breaker", () => {
		const nodeNamesById = new Map([
			["node-a", "Tokyo"],
			["node-b", "Amsterdam"],
			["node-c", "Tokyo"],
		]);

		expect(
			["node-a", "node-b", "node-c"].sort((first, second) =>
				compareNodeIdsByDisplayName(first, second, nodeNamesById),
			),
		).toEqual(["node-b", "node-a", "node-c"]);
		expect(nodeDisplayName("node-d", " ")).toBe("node-d");
	});
});
