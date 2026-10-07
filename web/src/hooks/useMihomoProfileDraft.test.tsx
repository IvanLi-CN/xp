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
	it("keeps an unloaded profile read-only until its data arrives", async () => {
		const saveProfile = vi.fn(
			async (profile: AdminUserMihomoProfile) => profile,
		);
		const { result, rerender } = renderHook(
			({ profile }: { profile: AdminUserMihomoProfile | undefined }) =>
				useMihomoProfileDraft({
					userId: "user-a",
					profile,
					saveProfile,
				}),
			{
				initialProps: {
					profile: undefined as AdminUserMihomoProfile | undefined,
				},
			},
		);

		expect(result.current.isLoaded).toBe(false);
		act(() => result.current.setField("mixin_yaml", "should not persist\n"));
		expect(result.current.draft).toEqual({
			mixin_yaml: "",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		});
		expect(result.current.dirty).toBe(false);
		await act(async () => {
			expect(await result.current.save()).toBe(false);
		});
		expect(saveProfile).not.toHaveBeenCalled();

		rerender({ profile: PROFILE_A });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "loaded edit\n"));
		expect(result.current.dirty).toBe(true);
	});

	it("preserves a dirty draft after a failed save and allows retry", async () => {
		const saveProfile = vi
			.fn<
				(profile: AdminUserMihomoProfile) => Promise<AdminUserMihomoProfile>
			>()
			.mockRejectedValueOnce(new Error("service unavailable"))
			.mockResolvedValueOnce({ ...PROFILE_A, mixin_yaml: "retry draft\n" });
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "retry draft\n"));
		await act(async () => {
			expect(await result.current.save()).toBe(false);
		});
		await waitFor(() =>
			expect(result.current.error).toBe("service unavailable"),
		);
		expect(result.current.draft.mixin_yaml).toBe("retry draft\n");
		expect(result.current.dirty).toBe(true);

		await act(async () => {
			expect(await result.current.save()).toBe(true);
		});
		expect(saveProfile).toHaveBeenCalledTimes(2);
		expect(result.current.dirty).toBe(false);
		expect(result.current.error).toBeNull();
	});

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
		const saveResolvers: Array<(profile: AdminUserMihomoProfile) => void> = [];
		const saveProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					saveResolvers.push(resolve);
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
		expect(result.current.isSaving).toBe(false);
		act(() => result.current.setField("mixin_yaml", "new draft\n"));
		let newSavePromise: Promise<boolean> | undefined;
		act(() => {
			newSavePromise = result.current.save();
		});
		expect(saveProfile).toHaveBeenCalledTimes(2);
		if (!newSavePromise) throw new Error("new save request was not created");
		await act(async () => {
			saveResolvers[1]?.({ ...PROFILE_B, mixin_yaml: "new draft\n" });
			expect(await newSavePromise).toBe(true);
		});

		act(() => {
			saveResolvers[0]?.({ ...PROFILE_A, mixin_yaml: "stale response\n" });
		});
		if (!savePromise) throw new Error("save request was not created");
		await savePromise;

		expect(result.current.draft).toEqual({
			...PROFILE_B,
			mixin_yaml: "new draft\n",
		});
		expect(result.current.baseline).toEqual({
			...PROFILE_B,
			mixin_yaml: "new draft\n",
		});
		expect(result.current.dirty).toBe(false);
	});

	it("ignores an old response after returning to the same user", async () => {
		const saveResolvers: Array<(profile: AdminUserMihomoProfile) => void> = [];
		const saveProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					saveResolvers.push(resolve);
				}),
		);
		const { result, rerender } = renderHook(
			({ userId, profile }) =>
				useMihomoProfileDraft({ userId, profile, saveProfile }),
			{ initialProps: { userId: "user-a", profile: PROFILE_A } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "first draft\n"));
		let firstSave: Promise<boolean> | undefined;
		act(() => {
			firstSave = result.current.save();
		});

		rerender({ userId: "user-b", profile: PROFILE_B });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		rerender({ userId: "user-a", profile: PROFILE_A });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "latest draft\n"));
		let secondSave: Promise<boolean> | undefined;
		act(() => {
			secondSave = result.current.save();
		});
		expect(saveProfile).toHaveBeenCalledTimes(2);
		if (!firstSave || !secondSave)
			throw new Error("save request was not created");

		await act(async () => {
			saveResolvers[0]?.({ ...PROFILE_A, mixin_yaml: "stale response\n" });
			expect(await firstSave).toBe(false);
		});
		expect(result.current.draft.mixin_yaml).toBe("latest draft\n");
		expect(result.current.isSaving).toBe(true);

		await act(async () => {
			saveResolvers[1]?.({ ...PROFILE_A, mixin_yaml: "latest draft\n" });
			expect(await secondSave).toBe(true);
		});
		expect(result.current.draft.mixin_yaml).toBe("latest draft\n");
		expect(result.current.dirty).toBe(false);
	});
});
