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
	sessionKey?: string;
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
	sessionKey,
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
	const sessionIdentity = `${userId}\u0000${sessionKey ?? userId}`;
	const currentSessionIdentityRef = useRef(sessionIdentity);
	const resetSessionIdentityRef = useRef(sessionIdentity);
	const userSessionRef = useRef(0);
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
		if (resetSessionIdentityRef.current === sessionIdentity) return;
		resetSessionIdentityRef.current = sessionIdentity;
		currentUserIdRef.current = userId;
		currentSessionIdentityRef.current = sessionIdentity;
		userSessionRef.current += 1;
		setIsSaving(false);
		savePromiseRef.current = null;
		setLoadedUserId(null);
		setDraftState(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		setBaseline(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		acceptedProfileKeyRef.current = profileKey(profile);
		lastSavedProfileKeyRef.current = null;
		setError(null);
	}, [profile, sessionIdentity, userId]);

	const incomingProfileKey = profileKey(profile);
	const isLoaded = loadedUserId === userId && profile !== undefined;
	useEffect(() => {
		if (
			!profile ||
			currentUserIdRef.current !== userId ||
			currentSessionIdentityRef.current !== sessionIdentity
		)
			return;
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
	}, [
		dirty,
		incomingProfileKey,
		isSaving,
		loadedUserId,
		profile,
		sessionIdentity,
		userId,
	]);

	const setField = useCallback(
		(documentId: MihomoProfileDocumentId, value: string) => {
			if (readOnly || isSaving || !isLoaded) return;
			setDraftState((current) => ({ ...current, [documentId]: value }));
			setError(null);
		},
		[isLoaded, isSaving, readOnly],
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
		if (
			readOnly ||
			!isLoaded ||
			!dirty ||
			currentUserIdRef.current !== userId ||
			currentSessionIdentityRef.current !== sessionIdentity
		) {
			return false;
		}
		if (savePromiseRef.current) return savePromiseRef.current;

		const targetUserId = userId;
		const targetSession = userSessionRef.current;
		const payload = cloneProfile(draft);
		setIsSaving(true);
		setError(null);
		const request = saveProfile(payload)
			.then((saved) => {
				if (
					currentUserIdRef.current !== targetUserId ||
					userSessionRef.current !== targetSession ||
					currentSessionIdentityRef.current !== sessionIdentity
				)
					return false;
				const next = cloneProfile(saved);
				setBaseline(next);
				setDraftState(next);
				setLoadedUserId(targetUserId);
				acceptedProfileKeyRef.current = profileKey(next);
				lastSavedProfileKeyRef.current = profileKey(next);
				return true;
			})
			.catch((saveError: unknown) => {
				if (
					currentUserIdRef.current === targetUserId &&
					userSessionRef.current === targetSession &&
					currentSessionIdentityRef.current === sessionIdentity
				) {
					setError(formatError(saveError));
				}
				return false;
			})
			.finally(() => {
				if (
					currentUserIdRef.current === targetUserId &&
					userSessionRef.current === targetSession &&
					currentSessionIdentityRef.current === sessionIdentity
				)
					setIsSaving(false);
				if (savePromiseRef.current === request) savePromiseRef.current = null;
			});
		savePromiseRef.current = request;
		return request;
	}, [
		dirty,
		draft,
		formatError,
		isLoaded,
		readOnly,
		saveProfile,
		sessionIdentity,
		userId,
	]);

	return {
		baseline,
		discard,
		draft,
		error,
		isLoaded,
		isSaving,
		save,
		setField,
		dirty,
	};
}
