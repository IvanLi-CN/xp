import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { PrimaryBackendProvider } from "./PrimaryBackendProvider";
import {
	clearPrimaryBackendDirectory,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";

afterEach(() => resetPrimaryBackendTransportForTests());

it("clears prior query data when the authentication session changes on the same backend", () => {
	const queryClient = new QueryClient();
	const view = render(
		<QueryClientProvider client={queryClient}>
			<PrimaryBackendProvider>
				<div>ready</div>
			</PrimaryBackendProvider>
		</QueryClientProvider>,
	);
	queryClient.setQueryData(["adminNodes"], ["old session data"]);
	act(() =>
		clearPrimaryBackendDirectory({ retainVerifiedPreAuthTarget: true }),
	);
	expect(queryClient.getQueryData(["adminNodes"])).toBeUndefined();
	view.unmount();
});
