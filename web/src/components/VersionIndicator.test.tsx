import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { UpgradeObservation } from "@/offline/upgradeObservation";

import { VersionIndicator } from "./VersionIndicator";

const TIMEOUT_MESSAGE =
	"Upgrade status could not be confirmed after one minute. Use Status to check again; " +
	"Upgrade remains locked until the node reports an idle or terminal state.";

function UpgradeHarness() {
	const [observation, setObservation] = useState<UpgradeObservation | null>(
		null,
	);
	const [terminal, setTerminal] = useState(false);
	return (
		<>
			<button
				type="button"
				onClick={() => {
					setObservation((current) =>
						current ? { ...current, phase: "timed_out" } : current,
					);
				}}
			>
				Simulate timeout
			</button>
			<button
				type="button"
				onClick={() => {
					setTerminal(true);
					setObservation((current) =>
						current ? { ...current, phase: "terminal" } : current,
					);
				}}
			>
				Record terminal
			</button>
			<VersionIndicator
				xpVersion="v3.21.8"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v3.21.10",
					checked_at: "2026-08-03T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					support: { supported: true, reason: null, trigger: "systemd" },
					status: {
						state: terminal ? "succeeded" : "idle",
						target_tag: terminal ? "v3.21.10" : null,
						repo: null,
						started_at: null,
						finished_at: null,
						exit_code: null,
						message: null,
						updated_at: "2026-08-03T00:00:00Z",
					},
				}}
				upgradeObservation={observation}
				onStartUpgrade={(targetTag) => {
					setObservation({
						targetTag,
						deadlineAtMs: Date.now() + 60_000,
						phase: "observing",
					});
				}}
				onDismissUpgradeResult={() => setObservation(null)}
			/>
		</>
	);
}

describe("<VersionIndicator /> upgrade observation", () => {
	it("shows managed cleanup without disabling a viable upgrade", () => {
		render(
			<VersionIndicator
				xpVersion="v3.21.8"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v3.21.10",
					checked_at: "2026-08-03T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					support: {
						supported: true,
						reason: null,
						trigger: "systemd",
						storage: {
							install: {
								path: "/usr/local/bin",
								available_bytes: 96 * 1024 * 1024,
								reclaimable_bytes: 48 * 1024 * 1024,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							workspace: {
								path: "/tmp/xp-ops",
								available_bytes: 256 * 1024 * 1024,
								reclaimable_bytes: 0,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							cleanup_required: true,
						},
					},
					status: {
						state: "idle",
						target_tag: null,
						repo: null,
						started_at: null,
						finished_at: null,
						exit_code: null,
						message: null,
						updated_at: "2026-08-03T00:00:00Z",
					},
				}}
			/>,
		);

		expect(
			screen.getByText("将自动清理历史升级文件后继续。"),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Upgrade" })).toBeEnabled();
	});

	it("blocks upgrade when cleanup cannot reach the space floor", () => {
		render(
			<VersionIndicator
				xpVersion="v3.21.8"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v3.21.10",
					checked_at: "2026-08-03T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					support: {
						supported: true,
						reason: null,
						trigger: "systemd",
						storage: {
							install: {
								path: "/usr/local/bin",
								available_bytes: 72 * 1024 * 1024,
								reclaimable_bytes: 8 * 1024 * 1024,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: false,
							},
							workspace: {
								path: "/tmp/xp-ops",
								available_bytes: 256 * 1024 * 1024,
								reclaimable_bytes: 0,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							cleanup_required: true,
						},
					},
					status: {
						state: "idle",
						target_tag: null,
						repo: null,
						started_at: null,
						finished_at: null,
						exit_code: null,
						message: null,
						updated_at: "2026-08-03T00:00:00Z",
					},
				}}
			/>,
		);

		expect(screen.getByText(/insufficient_upgrade_space/)).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Unavailable" })).toBeDisabled();
	});

	it("keeps the popover open and locks Upgrade after confirmation", async () => {
		render(<UpgradeHarness />);

		fireEvent.click(screen.getByRole("button", { name: "Upgrade" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Start upgrade" }),
		);

		expect(
			await screen.findByText(
				"Waiting for the node to reconnect and report the upgrade status.",
			),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Upgrading..." })).toBeDisabled();
	}, 10_000);

	it("does not reopen after manual close when observation state changes", async () => {
		render(<UpgradeHarness />);
		fireEvent.click(screen.getByRole("button", { name: "Upgrade" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Start upgrade" }),
		);
		await screen.findByText(
			"Waiting for the node to reconnect and report the upgrade status.",
		);

		fireEvent.keyDown(document, { key: "Escape" });
		expect(
			screen.queryByText(
				"Waiting for the node to reconnect and report the upgrade status.",
			),
		).not.toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "Simulate timeout" }));
		expect(screen.queryByText(TIMEOUT_MESSAGE)).not.toBeInTheDocument();
		fireEvent.click(
			screen.getByRole("button", {
				name: "Upgrade status could not be confirmed after one minute.",
			}),
		);

		expect(await screen.findByText(TIMEOUT_MESSAGE)).toBeInTheDocument();
	});

	it("keeps a terminal result open until the user closes it", async () => {
		render(<UpgradeHarness />);
		fireEvent.click(screen.getByRole("button", { name: "Upgrade" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Start upgrade" }),
		);
		fireEvent.click(screen.getByRole("button", { name: "Record terminal" }));

		const terminalMessage = await screen.findByText(
			"Last upgrade completed to v3.21.10.",
		);
		vi.useFakeTimers();
		try {
			fireEvent.pointerLeave(terminalMessage);
			act(() => vi.advanceTimersByTime(300));
			expect(terminalMessage).toBeInTheDocument();
		} finally {
			vi.useRealTimers();
		}
	});

	it("closes a terminal result with Escape", async () => {
		render(<UpgradeHarness />);
		fireEvent.click(screen.getByRole("button", { name: "Upgrade" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Start upgrade" }),
		);
		fireEvent.click(screen.getByRole("button", { name: "Record terminal" }));

		await screen.findByText("Last upgrade completed to v3.21.10.");
		fireEvent.keyDown(document, { key: "Escape" });

		expect(
			screen.queryByText("Last upgrade completed to v3.21.10."),
		).not.toBeInTheDocument();
	});
});
