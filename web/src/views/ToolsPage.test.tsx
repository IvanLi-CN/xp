import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { redactAdminMihomo } from "../api/adminTools";
import { UiPrefsProvider } from "../components/UiPrefs";
import { ToolsPage } from "./ToolsPage";

vi.mock("../api/adminTools");

const { mockReadAdminToken, mockWriteText } = vi.hoisted(() => ({
	mockReadAdminToken: vi.fn(() => "admintoken"),
	mockWriteText: vi.fn(async () => undefined),
}));

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: mockReadAdminToken,
	};
});

function renderPage() {
	return render(
		<UiPrefsProvider>
			<ToolsPage />
		</UiPrefsProvider>,
	);
}

describe("<ToolsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		mockReadAdminToken.mockReturnValue("admintoken");
		vi.mocked(redactAdminMihomo).mockResolvedValue({
			redacted_text: "server: e***.example.com\npassword: supe***cret\n",
		});
		Object.defineProperty(navigator, "clipboard", {
			value: {
				writeText: mockWriteText,
			},
			configurable: true,
		});
	});

	it("submits text payload with default format and level", async () => {
		renderPage();

		fireEvent.change(await screen.findByLabelText("Source text"), {
			target: {
				value:
					"vless://12345678-1234-1234-1234-123456789abc@example.com:443?pbk=public_key_value",
			},
		});
		fireEvent.click(await screen.findByRole("button", { name: "Run redact" }));

		await waitFor(() => {
			expect(redactAdminMihomo).toHaveBeenCalledWith("admintoken", {
				source_kind: "text",
				source:
					"vless://12345678-1234-1234-1234-123456789abc@example.com:443?pbk=public_key_value",
				level: "credentials",
				source_format: "auto",
			});
		});
	});

	it("switches to URL mode and sends url payload", async () => {
		renderPage();

		fireEvent.click(await screen.findByLabelText("Source kind"));
		fireEvent.click(await screen.findByRole("option", { name: "url" }));
		fireEvent.change(await screen.findByLabelText("Source URL"), {
			target: { value: "https://example.com/subscription" },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Run redact" }));

		await waitFor(() => {
			expect(redactAdminMihomo).toHaveBeenCalledWith("admintoken", {
				source_kind: "url",
				source: "https://example.com/subscription",
				level: "credentials",
				source_format: "auto",
			});
		});
	});

	it("renders preview on success and copies the result", async () => {
		renderPage();

		fireEvent.change(await screen.findByLabelText("Source text"), {
			target: { value: "server: edge.example.com\npassword: super-secret\n" },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Run redact" }));

		expect(await screen.findByLabelText("Redacted result")).toHaveValue(
			"server: e***.example.com\npassword: supe***cret\n",
		);

		fireEvent.click(
			await screen.findByRole("button", { name: "Copy redacted result" }),
		);

		await waitFor(() => {
			expect(mockWriteText).toHaveBeenCalledWith(
				"server: e***.example.com\npassword: supe***cret\n",
			);
		});
	});

	it("renders inline error state when execution fails", async () => {
		vi.mocked(redactAdminMihomo).mockRejectedValueOnce(
			new Error("request failed"),
		);
		renderPage();

		fireEvent.change(await screen.findByLabelText("Source text"), {
			target: { value: "server: edge.example.com\npassword: super-secret\n" },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Run redact" }));

		expect(await screen.findByText("request failed")).toBeTruthy();
		expect(screen.queryByLabelText("Redacted result")).toBeNull();
	});
});
