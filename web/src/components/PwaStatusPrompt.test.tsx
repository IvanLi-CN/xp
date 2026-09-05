import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	reloadAfterServiceWorkerUpdate: vi.fn().mockResolvedValue("reloaded"),
	setNeedRefresh: vi.fn(),
	setOfflineReady: vi.fn(),
	startServiceWorkerUpdatePolling: vi.fn(() => vi.fn()),
	updateServiceWorker: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("virtual:pwa-register/react", () => ({
	useRegisterSW: () => ({
		offlineReady: [false, mocks.setOfflineReady],
		needRefresh: [true, mocks.setNeedRefresh],
		updateServiceWorker: mocks.updateServiceWorker,
	}),
}));

vi.mock("../offline/serviceWorkerUpdates", () => ({
	reloadAfterServiceWorkerUpdate: mocks.reloadAfterServiceWorkerUpdate,
	startServiceWorkerUpdatePolling: mocks.startServiceWorkerUpdatePolling,
}));

import { PwaStatusPrompt } from "./PwaStatusPrompt";

describe("<PwaStatusPrompt />", () => {
	it("routes Reload through the update coordinator", async () => {
		const user = userEvent.setup();
		render(<PwaStatusPrompt />);

		await user.click(screen.getByRole("button", { name: "Reload" }));

		expect(mocks.reloadAfterServiceWorkerUpdate).toHaveBeenCalledWith(
			mocks.updateServiceWorker,
		);
	});
});
