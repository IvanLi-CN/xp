import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { TagInput } from "./TagInput";

function validateDomain(value: string): string | null {
	const trimmed = value.trim();
	if (!trimmed) return "required";
	if (/\s/.test(trimmed)) return "no spaces";
	if (trimmed.includes("://")) return "no scheme";
	if (trimmed.includes("/")) return "no path";
	if (trimmed.includes(":")) return "no port";
	if (trimmed.includes("*")) return "no wildcard";
	return null;
}

function Harness() {
	const [tags, setTags] = useState<string[]>([]);
	return (
		<TagInput
			label="serverNames"
			value={tags}
			onChange={setTags}
			validateTag={validateDomain}
			placeholder="oneclient.sfx.ms"
		/>
	);
}

describe("<TagInput />", () => {
	afterEach(() => cleanup());

	it("adds multiple tags from a comma-separated draft and allows make primary", () => {
		render(<Harness />);

		const input = screen.getByPlaceholderText("oneclient.sfx.ms");
		fireEvent.change(input, {
			target: { value: "a.example.com, b.example.com" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Add" }));

		expect(screen.getByText("a.example.com")).toBeInTheDocument();
		expect(screen.getByText("b.example.com")).toBeInTheDocument();
		// Primary marker should be shown on the first tag.
		expect(screen.getByText("primary")).toBeInTheDocument();

		// Make the 2nd tag primary.
		const makePrimaryButtons = screen.getAllByTitle("Make primary");
		expect(makePrimaryButtons.length).toBeGreaterThanOrEqual(1);
		const firstMakePrimary = makePrimaryButtons[0];
		if (!firstMakePrimary) {
			throw new Error("expected a Make primary button to exist");
		}
		fireEvent.click(firstMakePrimary);

		// Now b.example.com should be the primary tag and a.example.com should still exist.
		expect(screen.getByText("b.example.com")).toBeInTheDocument();
		expect(screen.getByText("a.example.com")).toBeInTheDocument();
	});

	it("rejects invalid tags and shows an error", () => {
		render(<Harness />);

		const input = screen.getByPlaceholderText("oneclient.sfx.ms");
		fireEvent.change(input, {
			target: { value: "https://example.com" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Add" }));

		expect(screen.queryByText("https://example.com")).toBeNull();
		expect(screen.getByRole("alert")).toBeInTheDocument();
	});
});
