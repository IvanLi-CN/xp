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
	});

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
});
