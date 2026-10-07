import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AdminUserMihomoProfile } from "../api/adminUsers";

export type MihomoProfileDocumentId = keyof AdminUserMihomoProfile;

export const MIHOMO_PROFILE_DOCUMENTS: readonly MihomoProfileDocumentId[] = [
	"mixin_yaml",
	"extra_proxies_yaml",
	"extra_proxy_providers_yaml",
];

export const EMPTY_MIHOMO_PROFILE: AdminUserMihomoProfile = {
	mixin_yaml: "",
	extra_proxies_yaml: "",
	extra_proxy_providers_yaml: "",
};

type UseMihomoProfileDraftProps = {
	userId: string;
	profile: AdminUserMihomoProfile | undefined;
	readOnly?: boolean;
	saveProfile: (
		profile: AdminUserMihomoProfile,
	) => Promise<AdminUserMihomoProfile>;
	formatError?: (error: unknown) => string;
};

function cloneProfile(profile: AdminUserMihomoProfile): AdminUserMihomoProfile {
	return { ...profile };
}

function profileKey(profile: AdminUserMihomoProfile | undefined): string {
	return profile ? JSON.stringify(profile) : "";
}

export function useMihomoProfileDraft({
	userId,
	profile,
	readOnly = false,
	saveProfile,
	formatError = (error) =>
		error instanceof Error ? error.message : String(error),
}: UseMihomoProfileDraftProps) {
	const [draft, setDraftState] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE),
	);
	const [baseline, setBaseline] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE),
	);
	const [loadedUserId, setLoadedUserId] = useState<string | null>(
		profile ? userId : null,
	);
	const [isSaving, setIsSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const currentUserIdRef = useRef(userId);
	const resetUserIdRef = useRef(userId);
	const savePromiseRef = useRef<Promise<boolean> | null>(null);
	const acceptedProfileKeyRef = useRef(profileKey(profile));
	const lastSavedProfileKeyRef = useRef<string | null>(null);

	const dirty = useMemo(
		() =>
			MIHOMO_PROFILE_DOCUMENTS.some(
				(documentId) => draft[documentId] !== baseline[documentId],
			),
		[draft, baseline],
	);

	useEffect(() => {
		if (resetUserIdRef.current === userId) return;
		resetUserIdRef.current = userId;
		currentUserIdRef.current = userId;
		setLoadedUserId(null);
		setDraftState(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		setBaseline(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		acceptedProfileKeyRef.current = profileKey(profile);
		lastSavedProfileKeyRef.current = null;
		setError(null);
	}, [profile, userId]);

	const incomingProfileKey = profileKey(profile);
	useEffect(() => {
		if (!profile || currentUserIdRef.current !== userId) return;
		if (loadedUserId !== userId) {
			setLoadedUserId(userId);
			setDraftState(cloneProfile(profile));
			setBaseline(cloneProfile(profile));
			acceptedProfileKeyRef.current = incomingProfileKey;
			setError(null);
			return;
		}
		if (incomingProfileKey === acceptedProfileKeyRef.current) {
			if (lastSavedProfileKeyRef.current === incomingProfileKey) {
				lastSavedProfileKeyRef.current = null;
			}
			return;
		}
		if (dirty || isSaving) {
			return;
		}
		if (lastSavedProfileKeyRef.current !== null) {
			if (incomingProfileKey === lastSavedProfileKeyRef.current) {
				lastSavedProfileKeyRef.current = null;
				acceptedProfileKeyRef.current = incomingProfileKey;
			}
			return;
		}
		setDraftState(cloneProfile(profile));
		setBaseline(cloneProfile(profile));
		acceptedProfileKeyRef.current = incomingProfileKey;
		setError(null);
	}, [dirty, incomingProfileKey, isSaving, loadedUserId, profile, userId]);

	const setField = useCallback(
		(documentId: MihomoProfileDocumentId, value: string) => {
			if (readOnly || isSaving) return;
			setDraftState((current) => ({ ...current, [documentId]: value }));
			setError(null);
		},
		[isSaving, readOnly],
	);

	const discard = useCallback(() => {
		if (isSaving) return;
		const next = profile ?? baseline;
		setDraftState(cloneProfile(next));
		setBaseline(cloneProfile(next));
		acceptedProfileKeyRef.current = profileKey(next);
		lastSavedProfileKeyRef.current = null;
		setLoadedUserId(profile ? userId : loadedUserId);
		setError(null);
	}, [baseline, isSaving, loadedUserId, profile, userId]);

	const save = useCallback(async (): Promise<boolean> => {
		if (readOnly || !dirty || currentUserIdRef.current !== userId) {
			return false;
		}
		if (savePromiseRef.current) return savePromiseRef.current;

		const targetUserId = userId;
		const payload = cloneProfile(draft);
		setIsSaving(true);
		setError(null);
		const request = saveProfile(payload)
			.then((saved) => {
				if (currentUserIdRef.current !== targetUserId) return false;
				const next = cloneProfile(saved);
				setBaseline(next);
				setDraftState(next);
				setLoadedUserId(targetUserId);
				acceptedProfileKeyRef.current = profileKey(next);
				lastSavedProfileKeyRef.current = profileKey(next);
				return true;
			})
			.catch((saveError: unknown) => {
				if (currentUserIdRef.current === targetUserId) {
					setError(formatError(saveError));
				}
				return false;
			})
			.finally(() => {
				if (currentUserIdRef.current === targetUserId) setIsSaving(false);
				if (savePromiseRef.current === request) savePromiseRef.current = null;
			});
		savePromiseRef.current = request;
		return request;
	}, [dirty, draft, formatError, readOnly, saveProfile, userId]);

	return {
		baseline,
		discard,
		draft,
		error,
		isLoaded: loadedUserId === userId && profile !== undefined,
		isSaving,
		save,
		setField,
		dirty,
	};
}
