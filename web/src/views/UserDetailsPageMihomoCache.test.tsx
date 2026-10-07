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
	mockFetchAdminUserMihomoProfile,
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

	it("serializes Mihomo saves before updating the cache", async () => {
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
		expect(mockPutAdminUserMihomoProfile).toHaveBeenCalledTimes(1);

		await act(async () => {
			saveResolvers[0]?.({
				mixin_yaml: "first save\n",
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: "",
			});
		});
		await waitFor(() =>
			expect(mockPutAdminUserMihomoProfile).toHaveBeenCalledTimes(2),
		);
		await act(async () => {
			saveResolvers[1]?.({
				mixin_yaml: "latest save\n",
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

	it("keeps a successful save ahead of an overlapping profile refetch", async () => {
		const initialProfile: AdminUserMihomoProfile = {
			mixin_yaml: "initial\n",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		};
		const savedProfile: AdminUserMihomoProfile = {
			...initialProfile,
			mixin_yaml: "saved successfully\n",
		};
		const staleRefetchProfile: AdminUserMihomoProfile = {
			...initialProfile,
			mixin_yaml: "stale refetch\n",
		};
		setupMocks({ mihomoProfile: initialProfile });
		let resolveRefetch: ((profile: AdminUserMihomoProfile) => void) | undefined;
		mockFetchAdminUserMihomoProfile
			.mockResolvedValueOnce(initialProfile)
			.mockResolvedValueOnce(initialProfile)
			.mockImplementationOnce(
				async () =>
					new Promise<AdminUserMihomoProfile>((resolve) => {
						resolveRefetch = resolve;
					}),
			);
		let resolveSave: ((profile: AdminUserMihomoProfile) => void) | undefined;
		mockPutAdminUserMihomoProfile.mockImplementation(
			async () =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveSave = resolve;
				}),
		);

		const page = renderPage();
		const editor = await within(page.container).findByLabelText("mixin_yaml");
		fireEvent.change(editor, { target: { value: "local save\n" } });
		fireEvent.click(
			within(page.container).getByRole("button", {
				name: "Save configuration",
			}),
		);
		await waitFor(() =>
			expect(mockPutAdminUserMihomoProfile).toHaveBeenCalledTimes(1),
		);

		const refetchPromise = page.queryClient.refetchQueries({
			queryKey: [
				"adminUserMihomoProfile",
				"admintoken",
				fixtureCatalog.identifier.userPrimary(),
			],
		});
		await waitFor(() =>
			expect(mockFetchAdminUserMihomoProfile).toHaveBeenCalledTimes(3),
		);
		await act(async () => {
			resolveSave?.(savedProfile);
		});
		await waitFor(() =>
			expect(
				page.queryClient.getQueryData<AdminUserMihomoProfile>([
					"adminUserMihomoProfile",
					"admintoken",
					fixtureCatalog.identifier.userPrimary(),
				]),
			).toEqual(savedProfile),
		);
		await act(async () => {
			resolveRefetch?.(staleRefetchProfile);
			await refetchPromise;
		});

		await waitFor(() =>
			expect(
				page.queryClient.getQueryData<AdminUserMihomoProfile>([
					"adminUserMihomoProfile",
					"admintoken",
					fixtureCatalog.identifier.userPrimary(),
				]),
			).toEqual(savedProfile),
		);
		expect(editor).toHaveTextContent("saved successfully");
		await waitFor(() =>
			expect(
				within(page.container).getByRole("button", {
					name: "Save configuration",
				}),
			).toBeDisabled(),
		);
	});

	it("rejects a stale full-profile save after another tab changes the baseline", async () => {
		const initialProfile: AdminUserMihomoProfile = {
			mixin_yaml: "initial\n",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		};
		const otherTabProfile: AdminUserMihomoProfile = {
			...initialProfile,
			extra_proxies_yaml: "- name: other-tab\n",
		};
		setupMocks({ mihomoProfile: initialProfile });
		mockFetchAdminUserMihomoProfile
			.mockResolvedValueOnce(initialProfile)
			.mockResolvedValueOnce(otherTabProfile);

		const page = renderPage();
		const editor = await within(page.container).findByLabelText("mixin_yaml");
		fireEvent.change(editor, { target: { value: "local draft\n" } });
		fireEvent.click(
			within(page.container).getByRole("button", {
				name: "Save configuration",
			}),
		);

		await waitFor(() =>
			expect(mockFetchAdminUserMihomoProfile).toHaveBeenCalledTimes(2),
		);
		expect(mockPutAdminUserMihomoProfile).not.toHaveBeenCalled();
		await waitFor(() =>
			expect(
				within(page.container).getByText(/changed elsewhere/),
			).toBeInTheDocument(),
		);
		expect(editor).toHaveTextContent("local draft");
	});
});
