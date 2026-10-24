import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AdminMihomoImportPreview } from "../api/adminUsers";
import { MihomoSubscriptionImportDialog } from "./MihomoSubscriptionImportDialog";

const PREVIEW: AdminMihomoImportPreview = {
	detected_source: "raw",
	added_proxies_yaml:
		"- name: imported\n  type: ss\n  server: import.example.com\n  port: 443\n",
	counts: { added: 1, skipped: 1, failed: 1, warnings: 1 },
	items: [
		{
			source_index: 1,
			status: "added",
			name: "imported",
			scheme: "ss",
			warnings: ["ignored_optional_parameter:plugin"],
		},
		{
			source_index: 2,
			status: "skipped",
			scheme: "ss",
			warnings: [],
			reason: "duplicate",
		},
		{
			source_index: 3,
			status: "failed",
			scheme: "ssr",
			warnings: [],
			reason: "unsupported scheme",
		},
	],
};

function renderDialog(
	props: Partial<
		React.ComponentProps<typeof MihomoSubscriptionImportDialog>
	> = {},
) {
	return render(
		<MihomoSubscriptionImportDialog
			open
			onOpenChange={vi.fn()}
			adminToken="admin-token"
			userId="user-1"
			existingExtraProxiesYaml={"# keep\n"}
			readOnly={false}
			onAdd={vi.fn()}
			previewRequest={vi.fn().mockResolvedValue(PREVIEW)}
			{...props}
		/>,
	);
}

describe("MihomoSubscriptionImportDialog", () => {
	it("previews a pasted source, reports partial results, and only adds explicitly", async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		const previewRequest = vi.fn().mockResolvedValue(PREVIEW);
		renderDialog({ onAdd, previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));

		await expect(
			screen.findByText("Preview report"),
		).resolves.toBeInTheDocument();
		expect(previewRequest).toHaveBeenCalledWith(
			"ss://example",
			"# keep\n",
			expect.any(AbortSignal),
		);
		expect(onAdd).not.toHaveBeenCalled();
		await expect(
			screen.findByText("unsupported scheme"),
		).resolves.toBeInTheDocument();
		await expect(screen.findByText("imported")).resolves.toBeInTheDocument();

		await user.click(screen.getByRole("button", { name: "Add valid nodes" }));
		expect(onAdd).toHaveBeenCalledWith(
			"# keep\n- name: imported\n  type: ss\n  server: import.example.com\n  port: 443\n",
		);
	});

	it("locks Add when the draft fingerprint changes after preview", async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		const previewRequest = vi.fn().mockResolvedValue(PREVIEW);
		const view = renderDialog({ onAdd, previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));
		await screen.findByText("Preview report");

		view.rerender(
			<MihomoSubscriptionImportDialog
				open
				onOpenChange={vi.fn()}
				adminToken="admin-token"
				userId="user-1"
				existingExtraProxiesYaml={"# changed\n"}
				readOnly={false}
				onAdd={onAdd}
				previewRequest={previewRequest}
			/>,
		);

		await waitFor(() =>
			expect(
				screen.getByText(/profile changed while this preview was open/i),
			).toBeInTheDocument(),
		);
		expect(
			screen.getByRole("button", { name: "Add valid nodes" }),
		).toBeDisabled();
		await user.click(screen.getByRole("button", { name: "Add valid nodes" }));
		expect(onAdd).not.toHaveBeenCalled();
	});

	it("invalidates a preview when the target user changes", async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		const previewRequest = vi.fn().mockResolvedValue(PREVIEW);
		const view = renderDialog({ onAdd, previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));
		await screen.findByText("Preview report");

		view.rerender(
			<MihomoSubscriptionImportDialog
				open
				onOpenChange={vi.fn()}
				adminToken="admin-token"
				userId="user-2"
				existingExtraProxiesYaml={"# keep\n"}
				readOnly={false}
				onAdd={onAdd}
				previewRequest={previewRequest}
			/>,
		);

		await waitFor(() =>
			expect(
				screen.getByRole("button", { name: "Add valid nodes" }),
			).toBeDisabled(),
		);
		expect(screen.getByText("Preview report")).toBeInTheDocument();
		expect(onAdd).not.toHaveBeenCalled();
	});
});
