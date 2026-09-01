import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { RepositoryStatusSummary } from "./HistoryRepositoryStatus";

describe("RepositoryStatusSummary", () => {
	it("distinguishes a recoverable source backlog from permanent gaps", () => {
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
									node_id: fixtureCatalog.nodeId.fixture32(),
									ed25519_public_key: "ed25519-key",
									x25519_relay_public_key: "x25519-key",
								},
								lifecycle: "ready",
								replica_converged: true,
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
							},
							runtime: {
								storage_mode: "sqlite",
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
								record_count: 1,
								segment_count: 1,
								gap_count: 0,
								history_truncated: false,
								last_verified_unix_seconds: null,
								last_anti_entropy_unix_seconds: null,
								last_deep_verification_unix_seconds: null,
								last_dynamic_relay_attempt_unix_seconds: null,
								source_delivery: {
									state: "backlogged",
									pending_segments: 37,
									pending_bytes: 128 * 1024,
									oldest_pending_cursor: "node-a/4/runtime/3993",
								},
							},
						},
					],
				}}
			/>,
		);

		expect(screen.getByText(/source backlog/)).toBeVisible();
		expect(screen.getByText(/37 segments/)).toBeVisible();
		expect(screen.queryByText("0 gaps")).not.toBeInTheDocument();
	});

	it("shows journal order repair while preserving the backlog count", () => {
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
									node_id: fixtureCatalog.nodeId.fixture32(),
									ed25519_public_key: "ed25519-key",
									x25519_relay_public_key: "x25519-key",
								},
								lifecycle: "ready",
								replica_converged: true,
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
							},
							runtime: {
								storage_mode: "sqlite",
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
								record_count: 1,
								segment_count: 1,
								gap_count: 0,
								history_truncated: false,
								last_verified_unix_seconds: null,
								last_anti_entropy_unix_seconds: null,
								last_deep_verification_unix_seconds: null,
								last_dynamic_relay_attempt_unix_seconds: null,
								source_delivery: {
									state: "journal_order_repairing",
									pending_segments: 20_000,
									pending_bytes: 128 * 1024 * 1024,
								},
							},
						},
					],
				}}
			/>,
		);

		expect(screen.getByText(/source journal order repair/)).toBeVisible();
		expect(screen.getByText(/20000 segments/)).toBeVisible();
	});

	it("distinguishes committed SQLite maintenance degradation from JSON fallback", () => {
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
									node_id: fixtureCatalog.nodeId.fixture32(),
									ed25519_public_key: "ed25519-key",
									x25519_relay_public_key: "x25519-key",
								},
								lifecycle: "ready",
								replica_converged: true,
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
							},
							runtime: {
								storage_mode: "sqlite_degraded",
								capacity: {
									quota_bytes: 10 * 1024 ** 3,
									used_bytes: 1024,
									filesystem_available_bytes: 1024 ** 3,
								},
								record_count: 1,
								segment_count: 1,
								gap_count: 0,
								history_truncated: false,
								last_verified_unix_seconds: null,
								last_anti_entropy_unix_seconds: null,
								last_deep_verification_unix_seconds: null,
								last_dynamic_relay_attempt_unix_seconds: null,
							},
						},
					],
				}}
			/>,
		);

		expect(screen.getByText(/SQLite maintenance degraded/)).toBeVisible();
		expect(screen.queryByText(/JSON fallback/)).not.toBeInTheDocument();
	});

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
									node_id: fixtureCatalog.nodeId.fixture32(),
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

	it("renders a known node name with its full identity", () => {
		const nodeId = fixtureCatalog.nodeId.fixture32();
		render(
			<RepositoryStatusSummary
				nodeNames={{ [fixtureCatalog.nodeId.fixture32()]: "edge-a" }}
				status={{
					configured: true,
					partial: false,
					unreachable_node_ids: [],
					items: [
						{
							member: {
								identity: {
									node_id: fixtureCatalog.nodeId.fixture32(),
									ed25519_public_key: "ed25519-key",
									x25519_relay_public_key: "x25519-key",
								},
								lifecycle: "ready",
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

		expect(screen.getByText("edge-a")).toBeVisible();
		expect(screen.getByText(nodeId)).toBeVisible();
	});

	it("labels an unmapped directory entry as unknown while retaining its ID", () => {
		const nodeId = fixtureCatalog.nodeId.fixture36();
		render(
			<RepositoryStatusSummary
				nodeNames={{}}
				status={{
					configured: true,
					partial: false,
					unreachable_node_ids: [],
					items: [
						{
							member: {
								identity: {
									node_id: fixtureCatalog.nodeId.fixture36(),
									ed25519_public_key: "ed25519-key",
									x25519_relay_public_key: "x25519-key",
								},
								lifecycle: "ready",
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

		expect(screen.getByText("Unknown node")).toBeVisible();
		expect(screen.getByText(nodeId)).toBeVisible();
	});
});
