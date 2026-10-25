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
	it("uses a bottom sheet surface that becomes a centered panel on larger screens", () => {
		renderDialog();

		expect(screen.getByRole("dialog")).toHaveClass(
			"grid",
			"h-auto",
			"bottom-0",
			"rounded-t-2xl",
			"sm:top-1/2",
			"sm:rounded-2xl",
		);
		expect(screen.getByRole("button", { name: "Close" })).toHaveClass(
			"size-11",
			"focus-visible:ring-[3px]",
		);
	});

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
		expect(screen.getByLabelText("Import counts")).toHaveTextContent(
			"Added1Skipped1Failed1Warnings1",
		);
		expect(screen.getByRole("status")).toHaveTextContent(
			"Preview complete. 1 added, 1 skipped, 1 failed, 1 warning.",
		);
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

	it("shows a preview error without changing the draft", async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		const previewRequest = vi
			.fn()
			.mockRejectedValue(new Error("preview unavailable"));
		renderDialog({ onAdd, previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));

		await expect(screen.findByRole("alert")).resolves.toHaveTextContent(
			"preview unavailable",
		);
		expect(onAdd).not.toHaveBeenCalled();
		expect(
			screen.getByRole("button", { name: "Add valid nodes" }),
		).toBeDisabled();
	});

	it("aborts a pending preview when cancelled", async () => {
		const user = userEvent.setup();
		const onOpenChange = vi.fn();
		let resolvePreview:
			| ((preview: AdminMihomoImportPreview) => void)
			| undefined;
		let requestSignal: AbortSignal | undefined;
		const pendingPreview = new Promise<AdminMihomoImportPreview>((resolve) => {
			resolvePreview = resolve;
		});
		const previewRequest = vi.fn(
			(_source: string, _existing: string, signal?: AbortSignal) => {
				requestSignal = signal;
				return pendingPreview;
			},
		);
		renderDialog({ onOpenChange, previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));
		await waitFor(() => expect(previewRequest).toHaveBeenCalled());
		expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();

		await user.click(screen.getByRole("button", { name: "Cancel" }));

		expect(requestSignal?.aborted).toBe(true);
		expect(onOpenChange).toHaveBeenCalledWith(false);
		resolvePreview?.(PREVIEW);
	});

	it("keeps long node names readable instead of truncating them", async () => {
		const user = userEvent.setup();
		const longName = `imported-${"very-long-node-name-".repeat(8)}`;
		const previewRequest = vi.fn().mockResolvedValue({
			...PREVIEW,
			items: [{ ...PREVIEW.items[0], name: longName }],
		});
		renderDialog({ previewRequest });

		await user.type(
			screen.getByLabelText("Subscription or node content"),
			"ss://example",
		);
		await user.click(screen.getByRole("button", { name: "Preview" }));

		const name = await screen.findByText(longName);
		expect(name).toHaveClass("break-words");
		expect(name).not.toHaveClass("truncate");
		expect(name).toHaveAttribute("title", longName);
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
