import {
	act,
	cleanup,
	fireEvent,
	waitFor,
	within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AdminUserMihomoProfile } from "../api/adminUsers";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	mockPutAdminUserMihomoProfile,
	mockReadAdminToken,
	mockUserId,
	renderPage,
	setupMocks,
} from "./UserDetailsPage.testSupport";

describe("<UserDetailsPage /> Mihomo cache", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		mockReadAdminToken.mockReturnValue("admintoken");
		mockUserId.mockReturnValue(fixtureCatalog.identifier.userPrimary());
	});

	afterEach(() => cleanup());

	it("does not let an older Mihomo save response replace a newer cache entry", async () => {
		setupMocks({
			mihomoProfile: {
				mixin_yaml: "initial\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			},
		});
		const saveResolvers: Array<(profile: AdminUserMihomoProfile) => void> = [];
		mockPutAdminUserMihomoProfile.mockImplementation(
			async () =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					saveResolvers.push(resolve);
				}),
		);

		const firstPage = renderPage();
		const firstEditor = await within(firstPage.container).findByLabelText(
			"mixin_yaml",
		);
		fireEvent.change(firstEditor, { target: { value: "first save\n" } });
		fireEvent.click(
			within(firstPage.container).getByRole("button", {
				name: "Save configuration",
			}),
		);
		await waitFor(() =>
			expect(mockPutAdminUserMihomoProfile).toHaveBeenCalledTimes(1),
		);

		firstPage.unmount();
		const secondPage = renderPage(firstPage.queryClient);
		const secondEditor = await within(secondPage.container).findByLabelText(
			"mixin_yaml",
		);
		fireEvent.change(secondEditor, { target: { value: "latest save\n" } });
		fireEvent.click(
			within(secondPage.container).getByRole("button", {
				name: "Save configuration",
			}),
		);
		await waitFor(() =>
			expect(mockPutAdminUserMihomoProfile).toHaveBeenCalledTimes(2),
		);

		await act(async () => {
			saveResolvers[1]?.({
				mixin_yaml: "latest save\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			});
			saveResolvers[0]?.({
				mixin_yaml: "stale save\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			});
		});

		expect(
			firstPage.queryClient.getQueryData<AdminUserMihomoProfile>([
				"adminUserMihomoProfile",
				"admintoken",
				fixtureCatalog.identifier.userPrimary(),
			]),
		).toEqual({
			mixin_yaml: "latest save\n",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		});
	});
});
