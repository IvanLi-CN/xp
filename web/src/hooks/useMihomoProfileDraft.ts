import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AdminUserMihomoProfile } from "../api/adminUsers";
import {
	type MihomoSaveDiagnostics,
	type MihomoSaveFeedback,
	isMihomoWriteOutcomeUnknown,
	mihomoProfilesEqual,
	mihomoSaveDiagnostics,
	submittedMihomoProfile,
} from "../utils/mihomoSaveOutcome";

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
		baseline: AdminUserMihomoProfile,
	) => Promise<AdminUserMihomoProfile>;
	verifyProfile?: (
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

export const MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX =
	"xp.mihomo-profile-draft.v1:";

type PersistedMihomoDraft = {
	draft: AdminUserMihomoProfile;
	saveFeedback: MihomoSaveFeedback | null;
};

function mihomoDraftStorageKey(userId: string): string {
	return `${MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX}${encodeURIComponent(userId)}`;
}

function validProfile(value: unknown): value is AdminUserMihomoProfile {
	return (
		typeof value === "object" &&
		value !== null &&
		MIHOMO_PROFILE_DOCUMENTS.every(
			(documentId) =>
				typeof (value as Record<string, unknown>)[documentId] === "string",
		)
	);
}

function safeDiagnostics(value: unknown): MihomoSaveDiagnostics {
	if (typeof value !== "object" || value === null) return {};
	const details = value as Record<string, unknown>;
	const diagnostics: MihomoSaveDiagnostics = {};
	for (const [source, target] of [
		["code", "code"],
		["cause", "cause"],
		["dispatchState", "dispatchState"],
		["supportId", "supportId"],
		["targetNodeId", "targetNodeId"],
	] as const) {
		const candidate = details[source];
		if (typeof candidate === "string" && candidate.length > 0) {
			diagnostics[target] = candidate;
		}
	}
	return diagnostics;
}

function persistedFeedback(value: unknown): MihomoSaveFeedback | null {
	if (typeof value !== "object" || value === null) return null;
	const feedback = value as Record<string, unknown>;
	const verification = feedback.verification;
	if (
		feedback.kind !== "outcome_unknown" ||
		(verification !== "matches" &&
			verification !== "different" &&
			verification !== "unavailable")
	)
		return null;
	return {
		kind: "outcome_unknown",
		verification,
		diagnostics: safeDiagnostics(feedback.diagnostics),
	};
}

function readPersistedMihomoDraft(userId: string): PersistedMihomoDraft | null {
	if (typeof window === "undefined") return null;
	try {
		const raw = window.localStorage.getItem(mihomoDraftStorageKey(userId));
		if (!raw) return null;
		const parsed: unknown = JSON.parse(raw);
		if (typeof parsed !== "object" || parsed === null) return null;
		const record = parsed as Record<string, unknown>;
		if (!validProfile(record.draft)) return null;
		return {
			draft: cloneProfile(record.draft),
			saveFeedback: persistedFeedback(record.saveFeedback),
		};
	} catch {
		return null;
	}
}

function persistMihomoDraft(
	userId: string,
	draft: AdminUserMihomoProfile,
	saveFeedback: MihomoSaveFeedback | null,
): void {
	if (typeof window === "undefined") return;
	try {
		window.localStorage.setItem(
			mihomoDraftStorageKey(userId),
			JSON.stringify({ draft, saveFeedback }),
		);
	} catch {
		// Browser storage can be unavailable or full; the in-memory draft remains authoritative.
	}
}

function clearPersistedMihomoDraft(userId: string): void {
	if (typeof window === "undefined") return;
	try {
		window.localStorage.removeItem(mihomoDraftStorageKey(userId));
	} catch {
		// Ignore storage failures; clearing local React state is still safe.
	}
}

export function useMihomoProfileDraft({
	userId,
	sessionKey,
	profile,
	readOnly = false,
	saveProfile,
	verifyProfile,
	formatError = (error) =>
		error instanceof Error ? error.message : String(error),
}: UseMihomoProfileDraftProps) {
	const storageKey = mihomoDraftStorageKey(userId);
	const initialPersistedDraft = readPersistedMihomoDraft(userId);
	const [draft, setDraftState] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(
			initialPersistedDraft?.draft ?? profile ?? EMPTY_MIHOMO_PROFILE,
		),
	);
	const [baseline, setBaseline] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE),
	);
	const [loadedUserId, setLoadedUserId] = useState<string | null>(
		profile ? userId : null,
	);
	const [isSaving, setIsSaving] = useState(false);
	const [isVerifying, setIsVerifying] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [saveFeedback, setSaveFeedback] = useState<MihomoSaveFeedback | null>(
		() => initialPersistedDraft?.saveFeedback ?? null,
	);
	const currentUserIdRef = useRef(userId);
	const sessionIdentity = `${userId}\u0000${sessionKey ?? userId}`;
	const currentSessionIdentityRef = useRef(sessionIdentity);
	const resetSessionIdentityRef = useRef(sessionIdentity);
	const userSessionRef = useRef(0);
	const hydratedStorageKeyRef = useRef(profile ? storageKey : null);
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
		setIsVerifying(false);
		savePromiseRef.current = null;
		setLoadedUserId(null);
		setDraftState(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		setBaseline(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
		hydratedStorageKeyRef.current = null;
		acceptedProfileKeyRef.current = profileKey(profile);
		lastSavedProfileKeyRef.current = null;
		setError(null);
		setSaveFeedback(null);
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
			const persisted = readPersistedMihomoDraft(userId);
			setLoadedUserId(userId);
			setDraftState(cloneProfile(persisted?.draft ?? profile));
			setBaseline(cloneProfile(profile));
			acceptedProfileKeyRef.current = incomingProfileKey;
			setError(null);
			setSaveFeedback(persisted?.saveFeedback ?? null);
			hydratedStorageKeyRef.current = storageKey;
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
		setSaveFeedback(null);
		clearPersistedMihomoDraft(userId);
	}, [
		dirty,
		incomingProfileKey,
		isSaving,
		loadedUserId,
		profile,
		sessionIdentity,
		storageKey,
		userId,
	]);

	useEffect(() => {
		if (!isLoaded || hydratedStorageKeyRef.current !== storageKey) return;
		if (!dirty && !saveFeedback) {
			clearPersistedMihomoDraft(userId);
			return;
		}
		persistMihomoDraft(userId, draft, saveFeedback);
	}, [draft, dirty, isLoaded, saveFeedback, storageKey, userId]);

	useEffect(() => {
		if (typeof window === "undefined") return;
		const handleStorage = (event: StorageEvent) => {
			if (
				event.key !== storageKey ||
				!isLoaded ||
				dirty ||
				isSaving ||
				isVerifying
			)
				return;
			const persisted = event.newValue
				? readPersistedMihomoDraft(userId)
				: null;
			const next =
				persisted?.draft ?? cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE);
			setDraftState(next);
			setBaseline(cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE));
			setSaveFeedback(persisted?.saveFeedback ?? null);
			setError(null);
			acceptedProfileKeyRef.current = profileKey(profile);
			lastSavedProfileKeyRef.current = null;
		};
		window.addEventListener("storage", handleStorage);
		return () => window.removeEventListener("storage", handleStorage);
	}, [dirty, isLoaded, isSaving, isVerifying, profile, storageKey, userId]);

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
		setSaveFeedback(null);
	}, [baseline, isSaving, loadedUserId, profile, userId]);

	const acceptProfile = useCallback(
		(saved: AdminUserMihomoProfile, targetUserId: string) => {
			const next = cloneProfile(saved);
			setBaseline(next);
			setDraftState(next);
			setLoadedUserId(targetUserId);
			acceptedProfileKeyRef.current = profileKey(next);
			lastSavedProfileKeyRef.current = profileKey(next);
		},
		[],
	);

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
				acceptProfile(saved, targetUserId);
				setSaveFeedback(null);
				return true;
			})
			.catch(async (saveError: unknown) => {
				if (
					currentUserIdRef.current === targetUserId &&
					userSessionRef.current === targetSession &&
					currentSessionIdentityRef.current === sessionIdentity
				) {
					if (isMihomoWriteOutcomeUnknown(saveError) && verifyProfile) {
						const submitted = submittedMihomoProfile(saveError) ?? payload;
						setIsVerifying(true);
						try {
							const latest = await verifyProfile(submitted);
							if (
								currentUserIdRef.current !== targetUserId ||
								userSessionRef.current !== targetSession ||
								currentSessionIdentityRef.current !== sessionIdentity
							)
								return false;
							if (mihomoProfilesEqual(latest, submitted)) {
								acceptProfile(latest, targetUserId);
								setError(null);
								setSaveFeedback({
									kind: "outcome_unknown",
									verification: "matches",
									diagnostics: mihomoSaveDiagnostics(saveError),
								});
								return true;
							}
							setSaveFeedback({
								kind: "outcome_unknown",
								verification: "different",
								diagnostics: mihomoSaveDiagnostics(saveError),
							});
						} catch {
							setSaveFeedback({
								kind: "outcome_unknown",
								verification: "unavailable",
								diagnostics: mihomoSaveDiagnostics(saveError),
							});
						} finally {
							setIsVerifying(false);
						}
					}
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
		acceptProfile,
		dirty,
		draft,
		formatError,
		isLoaded,
		readOnly,
		saveProfile,
		verifyProfile,
		sessionIdentity,
		baseline,
		userId,
	]);

	const verify = useCallback(async (): Promise<boolean> => {
		if (
			!verifyProfile ||
			readOnly ||
			!isLoaded ||
			isSaving ||
			currentUserIdRef.current !== userId ||
			currentSessionIdentityRef.current !== sessionIdentity
		)
			return false;
		const targetUserId = userId;
		const targetSession = userSessionRef.current;
		const submitted = cloneProfile(draft);
		setIsVerifying(true);
		setError(null);
		try {
			const latest = await verifyProfile(submitted);
			if (
				currentUserIdRef.current !== targetUserId ||
				userSessionRef.current !== targetSession ||
				currentSessionIdentityRef.current !== sessionIdentity
			)
				return false;
			if (mihomoProfilesEqual(latest, submitted)) {
				acceptProfile(latest, targetUserId);
				setSaveFeedback({
					kind: "outcome_unknown",
					verification: "matches",
					diagnostics: saveFeedback?.diagnostics ?? {},
				});
				return true;
			}
			setSaveFeedback({
				kind: "outcome_unknown",
				verification: "different",
				diagnostics: saveFeedback?.diagnostics ?? {},
			});
			setError(
				"Save result is still unknown. Read-back differs from your draft; review before retrying.",
			);
			return false;
		} catch (verificationError) {
			setSaveFeedback({
				kind: "outcome_unknown",
				verification: "unavailable",
				diagnostics: saveFeedback?.diagnostics ?? {},
			});
			setError(formatError(verificationError));
			return false;
		} finally {
			setIsVerifying(false);
		}
	}, [
		acceptProfile,
		draft,
		formatError,
		isLoaded,
		isSaving,
		readOnly,
		saveFeedback,
		sessionIdentity,
		userId,
		verifyProfile,
	]);

	return {
		baseline,
		discard,
		draft,
		error,
		isLoaded,
		isSaving,
		isVerifying,
		saveFeedback,
		save,
		setField,
		verify,
		dirty,
	};
}
