import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { RepositoryStatusSummary } from "./HistoryRepositoryStatus";

describe("RepositoryStatusSummary", () => {
	it("renders lifecycle convergence, identity fingerprints, and lifecycle timestamps", () => {
		render(
			<RepositoryStatusSummary
				status={{
					configured: true,
					partial: false,
					unreachable_node_ids: [],
					items: [
						{
							member: {
								identity: {
									node_id: "repository-a",
									ed25519_public_key: "ed25519-public-key-material",
									x25519_relay_public_key: "x25519-public-key-material",
								},
								lifecycle: "ready",
								catch_up_completed_at: 1_700_000_000,
								ready_at: 1_700_000_300,
								replica_converged: true,
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
							},
						},
					],
				}}
			/>,
		);

		expect(screen.getByText("converged")).toBeVisible();
		expect(screen.getByText("Signing public key")).toBeVisible();
		expect(screen.getByText("Relay public key")).toBeVisible();
		expect(screen.getByText("Caught up")).toBeVisible();
		expect(screen.getByText("Ready")).toBeVisible();
		expect(screen.getByTitle("ed25519-public-key-material")).toBeVisible();
	});
});
