import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { QueryRefreshError } from "./QueryRefreshError";

describe("<QueryRefreshError />", () => {
	it("keeps the cached report available and exposes retry", () => {
		const onRetry = vi.fn();
		render(
			<QueryRefreshError
				title="Traffic refresh failed"
				description="503 upstream unavailable"
				error={new Error("unavailable")}
				onRetry={onRetry}
			/>,
		);

		expect(screen.getByRole("alert")).toHaveTextContent(
			"Traffic refresh failed",
		);
		fireEvent.click(screen.getByRole("button", { name: "Retry" }));
		expect(onRetry).toHaveBeenCalledTimes(1);
	});
});
