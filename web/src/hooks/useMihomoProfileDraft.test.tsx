import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AdminUserMihomoProfile } from "../api/adminUsers";
import { useMihomoProfileDraft } from "./useMihomoProfileDraft";

const PROFILE_A: AdminUserMihomoProfile = {
	mixin_yaml: "port: 0\n",
	extra_proxies_yaml: "",
	extra_proxy_providers_yaml: "",
};

const PROFILE_B: AdminUserMihomoProfile = {
	mixin_yaml: "port: 1\n",
	extra_proxies_yaml: "- name: edge\n",
	extra_proxy_providers_yaml: "ProviderA:\n  type: http\n",
};

describe("useMihomoProfileDraft", () => {
	it("keeps dirty drafts across refreshes and accepts a complete save response", async () => {
		let resolveSave: ((profile: AdminUserMihomoProfile) => void) | undefined;
		const saveProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveSave = resolve;
				}),
		);
		const { result, rerender } = renderHook(
			({ profile }) =>
				useMihomoProfileDraft({
					userId: "user-a",
					profile,
					saveProfile,
				}),
			{ initialProps: { profile: PROFILE_A } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "port: 99\n"));
		expect(result.current.dirty).toBe(true);

		rerender({
			profile: { ...PROFILE_A, mixin_yaml: "port: 2\n" },
		});
		expect(result.current.draft.mixin_yaml).toBe("port: 99\n");

		let firstSave: Promise<boolean>;
		let secondSave: Promise<boolean>;
		await act(async () => {
			firstSave = result.current.save();
			secondSave = result.current.save();
			expect(saveProfile).toHaveBeenCalledTimes(1);
			resolveSave?.({ ...PROFILE_B, mixin_yaml: "port: 99\n" });
			expect(await firstSave).toBe(true);
			expect(await secondSave).toBe(true);
		});

		expect(result.current.dirty).toBe(false);
		expect(result.current.baseline).toEqual({
			...PROFILE_B,
			mixin_yaml: "port: 99\n",
		});
	});

	it("ignores a late save response after the user changes", async () => {
		let resolveSave: ((profile: AdminUserMihomoProfile) => void) | undefined;
		const saveProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveSave = resolve;
				}),
		);
		const { result, rerender } = renderHook(
			({ userId, profile }) =>
				useMihomoProfileDraft({ userId, profile, saveProfile }),
			{ initialProps: { userId: "user-a", profile: PROFILE_A } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "pending\n"));
		let savePromise: Promise<boolean> | undefined;
		act(() => {
			savePromise = result.current.save();
		});

		rerender({ userId: "user-b", profile: PROFILE_B });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => {
			resolveSave?.({ ...PROFILE_A, mixin_yaml: "stale response\n" });
		});
		if (!savePromise) throw new Error("save request was not created");
		await savePromise;

		expect(result.current.draft).toEqual(PROFILE_B);
		expect(result.current.baseline).toEqual(PROFILE_B);
		expect(result.current.dirty).toBe(false);
	});
});
