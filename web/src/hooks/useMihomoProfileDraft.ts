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
	clusterId?: string;
	userId: string;
	sessionKey?: string;
	profile: AdminUserMihomoProfile | undefined;
	readOnly?: boolean;
	saveProfile: (
		profile: AdminUserMihomoProfile,
		baseline: AdminUserMihomoProfile,
	) => Promise<AdminUserMihomoProfile>;
	formatError?: (error: unknown) => string;
};

export type MihomoProfileConflict = {
	server: AdminUserMihomoProfile;
	draftBaseline: AdminUserMihomoProfile;
};

type StoredMihomoDraft = {
	version: 1;
	clusterId: string;
	userId: string;
	profileObject: "mihomo-profile";
	draft: AdminUserMihomoProfile;
	baseline: AdminUserMihomoProfile;
};

const MIHOMO_DRAFT_SCHEMA_VERSION = 1;

function sessionDraftKey(clusterId: string, userId: string): string {
	return `xp-mihomo-draft:v${MIHOMO_DRAFT_SCHEMA_VERSION}:${clusterId}:${userId}:mihomo-profile`;
}

function readStoredDraft(
	clusterId: string | undefined,
	userId: string,
): { value: StoredMihomoDraft | null; error: string | null } {
	if (typeof window === "undefined" || !clusterId) {
		return { value: null, error: null };
	}
	try {
		const raw = window.sessionStorage.getItem(
			sessionDraftKey(clusterId, userId),
		);
		if (!raw) return { value: null, error: null };
		const parsed = JSON.parse(raw) as StoredMihomoDraft;
		if (
			parsed?.version !== 1 ||
			parsed.clusterId !== clusterId ||
			parsed.userId !== userId ||
			parsed.profileObject !== "mihomo-profile" ||
			!parsed.draft ||
			!parsed.baseline
		) {
			return {
				value: null,
				error: "Stored Mihomo draft was invalid and was ignored.",
			};
		}
		return { value: parsed, error: null };
	} catch {
		return {
			value: null,
			error: "Mihomo draft recovery is unavailable in this tab.",
		};
	}
}

function writeStoredDraft(
	clusterId: string | undefined,
	userId: string,
	draft: AdminUserMihomoProfile,
	baseline: AdminUserMihomoProfile,
): string | null {
	if (typeof window === "undefined" || !clusterId) return null;
	try {
		window.sessionStorage.setItem(
			sessionDraftKey(clusterId, userId),
			JSON.stringify({
				version: 1,
				clusterId,
				userId,
				profileObject: "mihomo-profile",
				draft,
				baseline,
			} satisfies StoredMihomoDraft),
		);
		return null;
	} catch {
		return "Mihomo draft could not be kept in this tab; keep the page open until you save.";
	}
}

function clearStoredDraft(
	clusterId: string | undefined,
	userId: string,
): string | null {
	if (typeof window === "undefined" || !clusterId) return null;
	try {
		window.sessionStorage.removeItem(sessionDraftKey(clusterId, userId));
		return null;
	} catch {
		return "Mihomo draft could not be cleared from this tab.";
	}
}

function cloneProfile(profile: AdminUserMihomoProfile): AdminUserMihomoProfile {
	return { ...profile };
}

function profileKey(profile: AdminUserMihomoProfile | undefined): string {
	return profile ? JSON.stringify(profile) : "";
}

export function useMihomoProfileDraft({
	clusterId,
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
	const [loadedUserId, setLoadedUserId] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [storageError, setStorageError] = useState<string | null>(null);
	const [conflict, setConflict] = useState<MihomoProfileConflict | null>(null);
	const currentUserIdRef = useRef(userId);
	const sessionIdentity = `${clusterId ?? ""}\u0000${userId}\u0000${sessionKey ?? userId}`;
	const currentSessionIdentityRef = useRef(sessionIdentity);
	const resetSessionIdentityRef = useRef(sessionIdentity);
	const userSessionRef = useRef(0);
	const savePromiseRef = useRef<Promise<boolean> | null>(null);
	const acceptedProfileKeyRef = useRef(profileKey(profile));
	const lastSavedProfileKeyRef = useRef<string | null>(null);
	const restoredSessionRef = useRef<string | null>(null);

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
		setStorageError(null);
		setConflict(null);
		restoredSessionRef.current = null;
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
			const recovery = readStoredDraft(clusterId, userId);
			const stored = recovery.value;
			if (recovery.error) setStorageError(recovery.error);
			if (stored) {
				setDraftState(cloneProfile(stored.draft));
				setBaseline(cloneProfile(profile));
				if (profileKey(stored.baseline) !== incomingProfileKey) {
					setConflict({
						server: cloneProfile(profile),
						draftBaseline: cloneProfile(stored.baseline),
					});
				}
			} else {
				setDraftState(cloneProfile(profile));
				setBaseline(cloneProfile(profile));
			}
			restoredSessionRef.current = sessionIdentity;
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
			if (!isSaving) {
				setConflict({
					server: cloneProfile(profile),
					draftBaseline: cloneProfile(baseline),
				});
			}
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
		baseline,
		clusterId,
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
		setConflict(null);
		setStorageError(clearStoredDraft(clusterId, userId));
	}, [baseline, clusterId, isSaving, loadedUserId, profile, userId]);

	const continueDraft = useCallback(() => {
		if (!conflict || isSaving) return;
		const nextBaseline = cloneProfile(conflict.server);
		setBaseline(nextBaseline);
		setConflict(null);
		setStorageError(writeStoredDraft(clusterId, userId, draft, nextBaseline));
	}, [clusterId, conflict, draft, isSaving, userId]);

	const useServer = useCallback(() => {
		if (!conflict || isSaving) return;
		const next = cloneProfile(conflict.server);
		setDraftState(next);
		setBaseline(next);
		acceptedProfileKeyRef.current = profileKey(next);
		lastSavedProfileKeyRef.current = null;
		setConflict(null);
		setError(null);
		setStorageError(clearStoredDraft(clusterId, userId));
	}, [clusterId, conflict, isSaving, userId]);

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
		const request = saveProfile(payload, baseline)
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
				setStorageError(clearStoredDraft(clusterId, userId));
				setConflict(null);
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
		baseline,
		userId,
		clusterId,
	]);

	useEffect(() => {
		if (
			!isLoaded ||
			!dirty ||
			isSaving ||
			restoredSessionRef.current !== sessionIdentity
		)
			return;
		setStorageError(writeStoredDraft(clusterId, userId, draft, baseline));
	}, [
		baseline,
		clusterId,
		dirty,
		draft,
		isLoaded,
		isSaving,
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
		conflict,
		continueDraft,
		storageError,
		useServer,
	};
}
