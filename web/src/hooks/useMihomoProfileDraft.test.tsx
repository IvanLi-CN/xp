import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AdminUserMihomoProfile } from "../api/adminUsers";
import {
	MihomoSaveOutcomeUnknownError,
	MihomoSavePreflightError,
} from "../utils/mihomoSaveOutcome";
import {
	MIHOMO_DRAFT_PERSISTENCE_WARNING,
	MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX,
	useMihomoProfileDraft,
} from "./useMihomoProfileDraft";

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
	beforeEach(() => {
		window.localStorage.clear();
	});

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

	it("does not read back when the save preflight failed", async () => {
		const verifyProfile = vi.fn(async () => PROFILE_A);
		const saveProfile = vi.fn(async () => {
			throw new MihomoSavePreflightError(new TypeError("offline"));
		});
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "preflight draft\n"));
		await act(async () => expect(await result.current.save()).toBe(false));

		expect(verifyProfile).not.toHaveBeenCalled();
		expect(result.current.saveFeedback).toBeNull();
		expect(result.current.error).toContain("Nothing was sent");
		expect(result.current.draft.mixin_yaml).toBe("preflight draft\n");
	});

	it("reports when browser storage cannot retain the draft", async () => {
		const setItem = vi
			.spyOn(Storage.prototype, "setItem")
			.mockImplementation(() => {
				throw new Error("quota exceeded");
			});
		const { result, unmount } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile: vi.fn(async (profile) => profile),
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "memory-only draft\n"));
		await waitFor(() =>
			expect(result.current.error).toBe(MIHOMO_DRAFT_PERSISTENCE_WARNING),
		);
		setItem.mockRestore();
		unmount();
	});

	it("checks an unknown write once and accepts a matching read-back", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "unknown write\n" };
		const saveProfile = vi
			.fn<
				(
					profile: AdminUserMihomoProfile,
					baseline: AdminUserMihomoProfile,
				) => Promise<AdminUserMihomoProfile>
			>()
			.mockRejectedValueOnce(
				new MihomoSaveOutcomeUnknownError(
					new TypeError("Failed to fetch"),
					submitted,
				),
			);
		const verifyProfile = vi.fn(async () => submitted);
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", submitted.mixin_yaml));
		await act(async () => expect(await result.current.save()).toBe(true));

		expect(saveProfile).toHaveBeenCalledTimes(1);
		expect(verifyProfile).toHaveBeenCalledTimes(1);
		expect(result.current.dirty).toBe(false);
		expect(result.current.saveFeedback?.verification).toBe("matches");
		expect(result.current.error).toBeNull();

		act(() =>
			result.current.setField("mixin_yaml", "edited after read-back\n"),
		);
		expect(result.current.dirty).toBe(true);
		expect(result.current.saveFeedback).toBeNull();
	});

	it("allows an explicitly confirmed retry after a different read-back", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "unknown write\n" };
		const saveProfile = vi
			.fn<
				(
					profile: AdminUserMihomoProfile,
					baseline: AdminUserMihomoProfile,
					allowOverwrite?: boolean,
				) => Promise<AdminUserMihomoProfile>
			>()
			.mockRejectedValueOnce(
				new MihomoSaveOutcomeUnknownError(
					new TypeError("Failed to fetch"),
					submitted,
				),
			)
			.mockResolvedValueOnce(submitted);
		const verifyProfile = vi.fn(async () => PROFILE_A);
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", submitted.mixin_yaml));
		await act(async () => expect(await result.current.save()).toBe(false));
		expect(result.current.saveFeedback?.verification).toBe("different");
		await act(async () => expect(await result.current.save(true)).toBe(true));
		expect(saveProfile).toHaveBeenLastCalledWith(submitted, PROFILE_A, true);
	});

	it("keeps the draft and permits a manual check after a different read-back", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "unknown write\n" };
		const saveProfile = vi.fn(async () => {
			throw new MihomoSaveOutcomeUnknownError(
				new TypeError("Failed to fetch"),
				submitted,
			);
		});
		const verifyProfile = vi
			.fn<
				(profile: AdminUserMihomoProfile) => Promise<AdminUserMihomoProfile>
			>()
			.mockResolvedValueOnce(PROFILE_A)
			.mockResolvedValueOnce(submitted);
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", submitted.mixin_yaml));
		await act(async () => expect(await result.current.save()).toBe(false));
		expect(result.current.draft).toEqual(submitted);
		expect(result.current.dirty).toBe(true);
		expect(result.current.saveFeedback?.verification).toBe("different");
		expect(verifyProfile).toHaveBeenCalledTimes(1);

		await act(async () => expect(await result.current.verify()).toBe(true));
		expect(verifyProfile).toHaveBeenCalledTimes(2);
		expect(result.current.dirty).toBe(false);
		expect(result.current.saveFeedback?.verification).toBe("matches");

		verifyProfile.mockRejectedValueOnce(new Error("read-back unavailable"));
		await act(async () => expect(await result.current.verify()).toBe(false));
		expect(result.current.saveFeedback?.verification).toBe("unavailable");
	});

	it("blocks edits while a manual check is pending", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "unknown write\n" };
		let resolveVerify: ((profile: AdminUserMihomoProfile) => void) | undefined;
		const verifyProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveVerify = resolve;
				}),
		);
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile: vi.fn(async (profile) => profile),
				verifyProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", submitted.mixin_yaml));
		let verifyPromise: Promise<boolean> | undefined;
		act(() => {
			verifyPromise = result.current.verify();
		});
		await waitFor(() => expect(result.current.isVerifying).toBe(true));
		act(() => result.current.setField("mixin_yaml", "edited while checking\n"));
		act(() => result.current.discard());
		expect(result.current.draft).toEqual(submitted);
		expect(await result.current.save()).toBe(false);

		act(() => resolveVerify?.(submitted));
		if (!verifyPromise) throw new Error("verification request was not created");
		await act(async () => expect(await verifyPromise).toBe(true));
		expect(result.current.draft).toEqual(submitted);
	});

	it("does not retain a conflict from a matching manual read-back", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "verified draft\n" };
		let resolveVerify: ((profile: AdminUserMihomoProfile) => void) | undefined;
		const verifyProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveVerify = resolve;
				}),
		);
		const { result, rerender } = renderHook(
			({ profile }: { profile: AdminUserMihomoProfile }) =>
				useMihomoProfileDraft({
					userId: "user-a",
					profile,
					saveProfile: vi.fn(async (next) => next),
					verifyProfile,
				}),
			{ initialProps: { profile: PROFILE_A } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", submitted.mixin_yaml));
		let verifyPromise: Promise<boolean> | undefined;
		act(() => {
			verifyPromise = result.current.verify();
		});
		await waitFor(() => expect(result.current.isVerifying).toBe(true));
		rerender({ profile: submitted });
		act(() => resolveVerify?.(submitted));
		if (!verifyPromise) throw new Error("verification request was not created");
		await act(async () => expect(await verifyPromise).toBe(true));

		expect(result.current.conflict).toBeNull();
		expect(result.current.dirty).toBe(false);
	});

	it("ignores a stale verification rejection after the session changes", async () => {
		let rejectVerify: ((error: unknown) => void) | undefined;
		const verifyProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((_, reject) => {
					rejectVerify = reject;
				}),
		);
		const { result, rerender } = renderHook(
			({ sessionKey }: { sessionKey: string }) =>
				useMihomoProfileDraft({
					userId: "user-a",
					sessionKey,
					profile: PROFILE_A,
					saveProfile: vi.fn(async (profile) => profile),
					verifyProfile,
				}),
			{ initialProps: { sessionKey: "session-a" } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		let verifyPromise: Promise<boolean> | undefined;
		act(() => {
			verifyPromise = result.current.verify();
		});
		await waitFor(() => expect(result.current.isVerifying).toBe(true));

		rerender({ sessionKey: "session-b" });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => rejectVerify?.(new Error("stale read-back failure")));
		if (!verifyPromise) throw new Error("verification request was not created");
		await act(async () => expect(await verifyPromise).toBe(false));

		expect(result.current.error).toBeNull();
		expect(result.current.saveFeedback).toBeNull();
		expect(result.current.isVerifying).toBe(false);
	});

	it("keeps an unavailable unknown result after remounting", async () => {
		const submitted = { ...PROFILE_A, mixin_yaml: "persisted unknown\n" };
		const saveProfile = vi.fn(async () => {
			throw new MihomoSaveOutcomeUnknownError(
				new TypeError("Failed to fetch"),
				submitted,
			);
		});
		const verifyProfile = vi.fn(async () => {
			throw new Error("read-back unavailable");
		});
		const first = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);

		await waitFor(() => expect(first.result.current.isLoaded).toBe(true));
		act(() =>
			first.result.current.setField("mixin_yaml", submitted.mixin_yaml),
		);
		await act(async () =>
			expect(await first.result.current.save()).toBe(false),
		);
		expect(first.result.current.saveFeedback?.verification).toBe("unavailable");
		first.unmount();

		const second = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
				verifyProfile,
			}),
		);
		await waitFor(() => expect(second.result.current.isLoaded).toBe(true));
		expect(second.result.current.draft).toEqual(submitted);
		expect(second.result.current.dirty).toBe(true);
		expect(second.result.current.saveFeedback?.verification).toBe(
			"unavailable",
		);
	});

	it("restores the original baseline with a persisted draft", async () => {
		const first = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile: vi.fn(async (profile) => profile),
			}),
		);
		await waitFor(() => expect(first.result.current.isLoaded).toBe(true));
		act(() => first.result.current.setField("mixin_yaml", "stored draft\n"));
		await waitFor(() => {
			const key = Object.keys(window.localStorage).find((candidate) =>
				candidate.startsWith(MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX),
			);
			expect(key).toBeDefined();
			const persisted = JSON.parse(
				window.localStorage.getItem(key ?? "") ?? "{}",
			);
			expect(persisted.baseline).toEqual(PROFILE_A);
		});
		first.unmount();

		const saveProfile = vi.fn(async (profile) => profile);
		const second = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_B,
				saveProfile,
			}),
		);
		await waitFor(() => expect(second.result.current.isLoaded).toBe(true));
		expect(second.result.current.draft.mixin_yaml).toBe("stored draft\n");
		expect(second.result.current.baseline).toEqual(PROFILE_A);

		await act(async () =>
			expect(await second.result.current.save()).toBe(true),
		);
		expect(saveProfile).toHaveBeenCalledWith(
			expect.objectContaining({ mixin_yaml: "stored draft\n" }),
			PROFILE_A,
			false,
		);
	});

	it("blocks legacy drafts that have no persisted baseline", async () => {
		const tabStorageKey = "xp.mihomo-profile.tab-id.v1";
		const tabId =
			window.sessionStorage.getItem(tabStorageKey) ?? "legacy-test-tab";
		window.sessionStorage.setItem(tabStorageKey, tabId);
		const storageKey = `${MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX}${encodeURIComponent(
			"user-a",
		)}:${encodeURIComponent(tabId)}`;
		window.localStorage.setItem(
			storageKey,
			JSON.stringify({
				draft: { ...PROFILE_A, mixin_yaml: "legacy draft\n" },
				saveFeedback: null,
			}),
		);
		const saveProfile = vi.fn(async (profile) => profile);
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		await act(async () => expect(await result.current.save()).toBe(false));
		expect(saveProfile).not.toHaveBeenCalled();
		expect(result.current.error).toContain("original profile is unavailable");
	});

	it("separates a duplicated tab claim before retaining its draft", async () => {
		const tabStorageKey = "xp.mihomo-profile.tab-id.v1";
		const originalTabId = window.sessionStorage.getItem(tabStorageKey);
		const { result, unmount } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile: vi.fn(async (profile) => profile),
			}),
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "duplicated draft\n"));
		const claimKey = await waitFor(() => {
			const key = Object.keys(window.localStorage).find((candidate) =>
				candidate.startsWith("xp.mihomo-profile.tab-claim.v1:"),
			);
			expect(key).toBeDefined();
			return key as string;
		});
		const foreignClaim = JSON.stringify({ instanceId: "foreign-tab" });
		act(() => {
			window.localStorage.setItem(claimKey, foreignClaim);
			window.dispatchEvent(
				new StorageEvent("storage", {
					key: claimKey,
					newValue: foreignClaim,
					storageArea: window.localStorage,
				}),
			);
		});

		await waitFor(() =>
			expect(window.sessionStorage.getItem(tabStorageKey)).not.toBe(
				originalTabId,
			),
		);
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		expect(result.current.draft.mixin_yaml).toBe("duplicated draft\n");
		unmount();
		if (originalTabId) {
			window.sessionStorage.setItem(tabStorageKey, originalTabId);
		}
	});

	it("adopts a clean draft written by another tab", async () => {
		const { result } = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile: vi.fn(async (profile) => profile),
			}),
		);
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		const next = { ...PROFILE_A, mixin_yaml: "from another tab\n" };
		const key = Object.keys(window.localStorage).find((candidate) =>
			candidate.includes("xp.mihomo-profile-draft.v1"),
		);
		const storageKey =
			key ??
			`${MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX}${encodeURIComponent("user-a")}:other-tab`;
		window.localStorage.setItem(
			storageKey,
			JSON.stringify({
				draft: next,
				saveFeedback: null,
			}),
		);
		act(() => {
			window.dispatchEvent(
				new StorageEvent("storage", {
					key: storageKey,
					newValue: window.localStorage.getItem(storageKey),
				}),
			);
		});
		await waitFor(() => expect(result.current.draft).toEqual(next));
		expect(result.current.dirty).toBe(true);
	});

	it("keeps another tab's draft when this tab saves", async () => {
		const saveProfile = vi.fn(async (profile) => profile);
		const first = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
			}),
		);
		await waitFor(() => expect(first.result.current.isLoaded).toBe(true));
		act(() => first.result.current.setField("mixin_yaml", "this tab\n"));

		const otherTabDraft = { ...PROFILE_A, mixin_yaml: "other tab\n" };
		const otherTabKey = [
			MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX,
			encodeURIComponent("user-a"),
			":other-tab",
		].join("");
		window.localStorage.setItem(
			otherTabKey,
			JSON.stringify({
				draft: otherTabDraft,
				saveFeedback: null,
			}),
		);
		await act(async () => expect(await first.result.current.save()).toBe(true));
		first.unmount();

		const second = renderHook(() =>
			useMihomoProfileDraft({
				userId: "user-a",
				profile: PROFILE_A,
				saveProfile,
			}),
		);
		await waitFor(() => expect(second.result.current.isLoaded).toBe(true));
		expect(second.result.current.draft).toEqual(otherTabDraft);
		expect(second.result.current.dirty).toBe(true);
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
		await waitFor(() => expect(result.current.conflict).not.toBeNull());
		act(() => result.current.continueDraft());

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

	it("ignores a late save response after the same user session changes", async () => {
		let resolveSave: ((profile: AdminUserMihomoProfile) => void) | undefined;
		const saveProfile = vi.fn(
			() =>
				new Promise<AdminUserMihomoProfile>((resolve) => {
					resolveSave = resolve;
				}),
		);
		const { result, rerender } = renderHook(
			({ sessionKey, profile }) =>
				useMihomoProfileDraft({
					userId: "user-a",
					sessionKey,
					profile,
					saveProfile,
				}),
			{
				initialProps: { sessionKey: "token-a", profile: PROFILE_A },
			},
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "pending\n"));
		let savePromise: Promise<boolean> | undefined;
		act(() => {
			savePromise = result.current.save();
		});

		rerender({ sessionKey: "token-b", profile: PROFILE_B });
		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		expect(result.current.draft.mixin_yaml).toBe("pending\n");
		expect(result.current.baseline).toEqual(PROFILE_A);
		act(() => resolveSave?.({ ...PROFILE_A, mixin_yaml: "stale\n" }));
		if (!savePromise) throw new Error("save request was not created");
		expect(await savePromise).toBe(false);
		expect(result.current.draft).toEqual({
			...PROFILE_A,
			mixin_yaml: "pending\n",
		});
		expect(result.current.baseline).toEqual(PROFILE_A);
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

	it("recovers a same-tab draft and requires an explicit conflict choice", async () => {
		window.sessionStorage.clear();
		const saveProfile = vi.fn(
			async (profile: AdminUserMihomoProfile) => profile,
		);
		const first = renderHook(() =>
			useMihomoProfileDraft({
				clusterId: "cluster-recovery",
				userId: "user-recovery",
				profile: PROFILE_A,
				saveProfile,
			}),
		);
		await waitFor(() => expect(first.result.current.isLoaded).toBe(true));
		act(() => {
			first.result.current.setField("mixin_yaml", "draft after reload\n");
			first.result.current.setField(
				"extra_proxies_yaml",
				"- name: same-turn-edit\n",
			);
		});
		expect(
			window.sessionStorage.getItem(
				"xp-mihomo-draft:v1:cluster-recovery:user-recovery:mihomo-profile",
			),
		).toContain("draft after reload");
		expect(first.result.current.draft).toMatchObject({
			mixin_yaml: "draft after reload\n",
			extra_proxies_yaml: "- name: same-turn-edit\n",
		});
		await waitFor(() => expect(first.result.current.dirty).toBe(true));
		first.unmount();

		const second = renderHook(() =>
			useMihomoProfileDraft({
				clusterId: "cluster-recovery",
				userId: "user-recovery",
				profile: { ...PROFILE_A, mixin_yaml: "server changed\n" },
				saveProfile,
			}),
		);
		await waitFor(() => expect(second.result.current.conflict).not.toBeNull());
		expect(second.result.current.draft.mixin_yaml).toBe("draft after reload\n");
		await act(async () => {
			expect(await second.result.current.save()).toBe(false);
		});
		expect(saveProfile).toHaveBeenCalledTimes(0);
		act(() => second.result.current.continueDraft());
		expect(second.result.current.conflict).toBeNull();
		expect(second.result.current.dirty).toBe(true);
		await act(async () => {
			expect(await second.result.current.save()).toBe(true);
		});
		expect(
			window.sessionStorage.getItem(
				"xp-mihomo-draft:v1:cluster-recovery:user-recovery:mihomo-profile",
			),
		).toBeNull();
	});

	it("keeps a live-update conflict resolved after continuing the draft", async () => {
		const saveProfile = vi.fn(
			async (profile: AdminUserMihomoProfile) => profile,
		);
		const { result, rerender } = renderHook(
			({ profile }: { profile: AdminUserMihomoProfile }) =>
				useMihomoProfileDraft({
					userId: "user-live-conflict",
					profile,
					saveProfile,
				}),
			{ initialProps: { profile: PROFILE_A } },
		);

		await waitFor(() => expect(result.current.isLoaded).toBe(true));
		act(() => result.current.setField("mixin_yaml", "local draft\n"));
		rerender({ profile: PROFILE_B });
		await waitFor(() => expect(result.current.conflict).not.toBeNull());

		act(() => result.current.continueDraft());
		await waitFor(() => expect(result.current.conflict).toBeNull());
		expect(result.current.baseline).toEqual(PROFILE_B);
		expect(result.current.draft.mixin_yaml).toBe("local draft\n");
		expect(result.current.dirty).toBe(true);
	});

	it("uses the server version only after an explicit conflict choice", async () => {
		window.sessionStorage.clear();
		const saveProfile = vi.fn(
			async (profile: AdminUserMihomoProfile) => profile,
		);
		const first = renderHook(() =>
			useMihomoProfileDraft({
				clusterId: "cluster-server-choice",
				userId: "user-server-choice",
				profile: PROFILE_A,
				saveProfile,
			}),
		);
		await waitFor(() => expect(first.result.current.isLoaded).toBe(true));
		act(() => first.result.current.setField("mixin_yaml", "local draft\n"));
		first.unmount();

		const second = renderHook(() =>
			useMihomoProfileDraft({
				clusterId: "cluster-server-choice",
				userId: "user-server-choice",
				profile: PROFILE_B,
				saveProfile,
			}),
		);
		await waitFor(() => expect(second.result.current.conflict).not.toBeNull());
		act(() => second.result.current.useServer());
		expect(second.result.current.draft).toEqual(PROFILE_B);
		expect(second.result.current.baseline).toEqual(PROFILE_B);
		expect(second.result.current.dirty).toBe(false);
		expect(second.result.current.conflict).toBeNull();
		expect(
			window.sessionStorage.getItem(
				"xp-mihomo-draft:v1:cluster-server-choice:user-server-choice:mihomo-profile",
			),
		).toBeNull();
	});
});
