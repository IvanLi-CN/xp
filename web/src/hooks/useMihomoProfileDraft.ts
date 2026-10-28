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
const MIHOMO_PROFILE_TAB_ID_STORAGE_KEY = "xp.mihomo-profile.tab-id.v1";
const MIHOMO_PROFILE_TAB_CLAIM_STORAGE_KEY_PREFIX =
	"xp.mihomo-profile.tab-claim.v1:";
let mihomoBrowserTabInstanceId: string | null = null;

function randomBrowserIdentifier(): string {
	return typeof crypto !== "undefined" && "randomUUID" in crypto
		? crypto.randomUUID()
		: `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

type PersistedMihomoDraft = {
	draft: AdminUserMihomoProfile;
	baseline: AdminUserMihomoProfile | null;
	saveFeedback: MihomoSaveFeedback | null;
	updatedAt: number;
};

function browserTabId(): string {
	if (typeof window === "undefined") return "server";
	try {
		const existing = window.sessionStorage.getItem(
			MIHOMO_PROFILE_TAB_ID_STORAGE_KEY,
		);
		if (existing) return existing;
		const generated = randomBrowserIdentifier();
		window.sessionStorage.setItem(MIHOMO_PROFILE_TAB_ID_STORAGE_KEY, generated);
		return generated;
	} catch {
		return `ephemeral-${Date.now()}-${Math.random().toString(36).slice(2)}`;
	}
}

function browserTabInstanceId(): string {
	if (mihomoBrowserTabInstanceId) return mihomoBrowserTabInstanceId;
	const generated = randomBrowserIdentifier();
	mihomoBrowserTabInstanceId = generated;
	return generated;
}

function mihomoTabClaimStorageKey(tabId: string): string {
	return `${MIHOMO_PROFILE_TAB_CLAIM_STORAGE_KEY_PREFIX}${encodeURIComponent(tabId)}`;
}

function tabClaimInstanceId(value: string | null): string | null {
	if (!value) return null;
	try {
		const parsed: unknown = JSON.parse(value);
		if (typeof parsed !== "object" || parsed === null) return null;
		const instanceId = (parsed as Record<string, unknown>).instanceId;
		return typeof instanceId === "string" && instanceId.length > 0
			? instanceId
			: null;
	} catch {
		return null;
	}
}

function mihomoDraftStoragePrefix(userId: string): string {
	return `${MIHOMO_PROFILE_DRAFT_STORAGE_KEY_PREFIX}${encodeURIComponent(userId)}:`;
}

function mihomoDraftStorageKey(userId: string, tabId: string): string {
	return `${mihomoDraftStoragePrefix(userId)}${encodeURIComponent(tabId)}`;
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

function parsePersistedMihomoDraft(
	raw: string | null,
): PersistedMihomoDraft | null {
	if (!raw) return null;
	try {
		const parsed: unknown = JSON.parse(raw);
		if (typeof parsed !== "object" || parsed === null) return null;
		const record = parsed as Record<string, unknown>;
		if (!validProfile(record.draft)) return null;
		return {
			draft: cloneProfile(record.draft),
			baseline: validProfile(record.baseline)
				? cloneProfile(record.baseline)
				: null,
			saveFeedback: persistedFeedback(record.saveFeedback),
			updatedAt:
				typeof record.updatedAt === "number" &&
				Number.isFinite(record.updatedAt)
					? record.updatedAt
					: 0,
		};
	} catch {
		return null;
	}
}

function readPersistedMihomoDraft(
	storageKey: string,
): PersistedMihomoDraft | null {
	if (typeof window === "undefined") return null;
	try {
		return parsePersistedMihomoDraft(window.localStorage.getItem(storageKey));
	} catch {
		return null;
	}
}

function readLatestOtherMihomoDraft(
	userId: string,
	tabId: string,
): PersistedMihomoDraft | null {
	if (typeof window === "undefined") return null;
	const prefix = mihomoDraftStoragePrefix(userId);
	let latest: PersistedMihomoDraft | null = null;
	try {
		for (let index = 0; index < window.localStorage.length; index += 1) {
			const key = window.localStorage.key(index);
			if (!key || key === mihomoDraftStorageKey(userId, tabId)) continue;
			if (!key.startsWith(prefix)) continue;
			const candidate = parsePersistedMihomoDraft(
				window.localStorage.getItem(key),
			);
			if (candidate && (!latest || candidate.updatedAt > latest.updatedAt)) {
				latest = candidate;
			}
		}
	} catch {
		return latest;
	}
	return latest;
}

function persistMihomoDraft(
	storageKey: string,
	draft: AdminUserMihomoProfile,
	baseline: AdminUserMihomoProfile | null,
	saveFeedback: MihomoSaveFeedback | null,
): void {
	if (typeof window === "undefined") return;
	try {
		window.localStorage.setItem(
			storageKey,
			JSON.stringify({ draft, baseline, saveFeedback, updatedAt: Date.now() }),
		);
	} catch {
		// Browser storage can be unavailable or full; the in-memory draft remains authoritative.
	}
}

function clearPersistedMihomoDraft(storageKey: string): void {
	if (typeof window === "undefined") return;
	try {
		window.localStorage.removeItem(storageKey);
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
	const [tabId, setTabId] = useState(browserTabId);
	const tabInstanceIdRef = useRef<string | null>(null);
	if (tabInstanceIdRef.current === null) {
		tabInstanceIdRef.current = browserTabInstanceId();
	}
	const tabInstanceId = tabInstanceIdRef.current;
	const storageKey = mihomoDraftStorageKey(userId, tabId);
	const storagePrefix = mihomoDraftStoragePrefix(userId);
	const initialPersistedDraft =
		readPersistedMihomoDraft(storageKey) ??
		readLatestOtherMihomoDraft(userId, tabId);
	const initialBaseline =
		initialPersistedDraft?.baseline ?? profile ?? EMPTY_MIHOMO_PROFILE;
	const [draft, setDraftState] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(
			initialPersistedDraft?.draft ?? profile ?? EMPTY_MIHOMO_PROFILE,
		),
	);
	const [baseline, setBaseline] = useState<AdminUserMihomoProfile>(() =>
		cloneProfile(initialBaseline),
	);
	const [baselineKnown, setBaselineKnown] = useState(
		!initialPersistedDraft || initialPersistedDraft.baseline !== null,
	);
	const [loadedUserId, setLoadedUserId] = useState<string | null>(
		profile ? userId : null,
	);
	const incomingProfileKey = profileKey(profile);
	const isLoaded = loadedUserId === userId && profile !== undefined;
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
	const draftRef = useRef(draft);
	const baselineRef = useRef(baseline);
	const baselineKnownRef = useRef(baselineKnown);
	const saveFeedbackRef = useRef(saveFeedback);
	const isLoadedRef = useRef(isLoaded);
	draftRef.current = draft;
	baselineRef.current = baseline;
	baselineKnownRef.current = baselineKnown;
	saveFeedbackRef.current = saveFeedback;
	isLoadedRef.current = isLoaded;

	const dirty = useMemo(
		() =>
			MIHOMO_PROFILE_DOCUMENTS.some(
				(documentId) => draft[documentId] !== baseline[documentId],
			),
		[draft, baseline],
	);

	useEffect(() => {
		if (typeof window === "undefined") return;
		const claimKey = mihomoTabClaimStorageKey(tabId);
		const claim = (): boolean => {
			try {
				window.localStorage.setItem(
					claimKey,
					JSON.stringify({ instanceId: tabInstanceId, updatedAt: Date.now() }),
				);
				return true;
			} catch {
				// Browser storage can be unavailable; the current tab ID remains usable.
				return false;
			}
		};
		const rotateIfClaimLost = () => {
			try {
				if (
					tabClaimInstanceId(window.localStorage.getItem(claimKey)) ===
					tabInstanceId
				)
					return;
				const nextTabId = randomBrowserIdentifier();
				if (isLoadedRef.current && (dirty || saveFeedbackRef.current)) {
					persistMihomoDraft(
						mihomoDraftStorageKey(userId, nextTabId),
						draftRef.current,
						baselineKnownRef.current ? baselineRef.current : null,
						saveFeedbackRef.current,
					);
				}
				try {
					window.sessionStorage.setItem(
						MIHOMO_PROFILE_TAB_ID_STORAGE_KEY,
						nextTabId,
					);
				} catch {
					// Continue with the in-memory tab ID when session storage is unavailable.
				}
				setTabId(nextTabId);
				hydratedStorageKeyRef.current = null;
				setLoadedUserId(null);
			} catch {
				// Browser storage can be unavailable; the current tab ID remains usable.
			}
		};
		const claimEstablished = claim();
		if (claimEstablished) rotateIfClaimLost();
		const handleStorage = (event: StorageEvent) => {
			if (claimEstablished && event.key === claimKey && event.newValue) {
				rotateIfClaimLost();
			}
		};
		window.addEventListener("storage", handleStorage);
		return () => window.removeEventListener("storage", handleStorage);
	}, [dirty, tabId, tabInstanceId, userId]);

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
		setBaselineKnown(true);
		hydratedStorageKeyRef.current = null;
		acceptedProfileKeyRef.current = profileKey(profile);
		lastSavedProfileKeyRef.current = null;
		setError(null);
		setSaveFeedback(null);
	}, [profile, sessionIdentity, userId]);

	useEffect(() => {
		if (
			!profile ||
			currentUserIdRef.current !== userId ||
			currentSessionIdentityRef.current !== sessionIdentity
		)
			return;
		if (loadedUserId !== userId) {
			const persisted =
				readPersistedMihomoDraft(storageKey) ??
				readLatestOtherMihomoDraft(userId, tabId);
			setLoadedUserId(userId);
			setDraftState(cloneProfile(persisted?.draft ?? profile));
			setBaseline(cloneProfile(persisted?.baseline ?? profile));
			setBaselineKnown(!persisted || persisted.baseline !== null);
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
		setBaselineKnown(true);
		acceptedProfileKeyRef.current = incomingProfileKey;
		setError(null);
		setSaveFeedback(null);
		clearPersistedMihomoDraft(storageKey);
	}, [
		dirty,
		incomingProfileKey,
		isSaving,
		loadedUserId,
		profile,
		sessionIdentity,
		storageKey,
		tabId,
		userId,
	]);

	useEffect(() => {
		if (!isLoaded || hydratedStorageKeyRef.current !== storageKey) return;
		if (!dirty && !saveFeedback) {
			clearPersistedMihomoDraft(storageKey);
			return;
		}
		persistMihomoDraft(
			storageKey,
			draft,
			baselineKnown ? baseline : null,
			saveFeedback,
		);
	}, [
		baseline,
		baselineKnown,
		draft,
		dirty,
		isLoaded,
		saveFeedback,
		storageKey,
	]);

	useEffect(() => {
		if (typeof window === "undefined") return;
		const handleStorage = (event: StorageEvent) => {
			if (
				!event.key ||
				!event.key.startsWith(storagePrefix) ||
				event.key === storageKey ||
				!isLoaded ||
				dirty ||
				isSaving ||
				isVerifying
			)
				return;
			const persisted =
				parsePersistedMihomoDraft(event.newValue) ??
				readLatestOtherMihomoDraft(userId, tabId);
			const next =
				persisted?.draft ?? cloneProfile(profile ?? EMPTY_MIHOMO_PROFILE);
			setDraftState(next);
			setBaseline(
				cloneProfile(persisted?.baseline ?? profile ?? EMPTY_MIHOMO_PROFILE),
			);
			setBaselineKnown(!persisted || persisted.baseline !== null);
			setSaveFeedback(persisted?.saveFeedback ?? null);
			setError(null);
			acceptedProfileKeyRef.current = profileKey(profile);
			lastSavedProfileKeyRef.current = null;
		};
		window.addEventListener("storage", handleStorage);
		return () => window.removeEventListener("storage", handleStorage);
	}, [
		dirty,
		isLoaded,
		isSaving,
		isVerifying,
		profile,
		storageKey,
		storagePrefix,
		tabId,
		userId,
	]);

	const setField = useCallback(
		(documentId: MihomoProfileDocumentId, value: string) => {
			if (readOnly || isSaving || isVerifying || !isLoaded) return;
			setDraftState((current) => ({ ...current, [documentId]: value }));
			setError(null);
		},
		[isLoaded, isSaving, isVerifying, readOnly],
	);

	const discard = useCallback(() => {
		if (isSaving || isVerifying) return;
		const next = profile ?? baseline;
		setDraftState(cloneProfile(next));
		setBaseline(cloneProfile(next));
		setBaselineKnown(true);
		acceptedProfileKeyRef.current = profileKey(next);
		lastSavedProfileKeyRef.current = null;
		setLoadedUserId(profile ? userId : loadedUserId);
		setError(null);
		setSaveFeedback(null);
	}, [baseline, isSaving, isVerifying, loadedUserId, profile, userId]);

	const acceptProfile = useCallback(
		(saved: AdminUserMihomoProfile, targetUserId: string) => {
			const next = cloneProfile(saved);
			setBaseline(next);
			setBaselineKnown(true);
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
			isVerifying ||
			!dirty ||
			currentUserIdRef.current !== userId ||
			currentSessionIdentityRef.current !== sessionIdentity
		) {
			return false;
		}
		if (!baselineKnown) {
			setError(
				"This draft's original profile is unavailable. Review or discard it before saving.",
			);
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
							if (
								currentUserIdRef.current !== targetUserId ||
								userSessionRef.current !== targetSession ||
								currentSessionIdentityRef.current !== sessionIdentity
							)
								return false;
							setSaveFeedback({
								kind: "outcome_unknown",
								verification: "unavailable",
								diagnostics: mihomoSaveDiagnostics(saveError),
							});
						} finally {
							if (
								currentUserIdRef.current === targetUserId &&
								userSessionRef.current === targetSession &&
								currentSessionIdentityRef.current === sessionIdentity
							)
								setIsVerifying(false);
						}
					}
					if (
						currentUserIdRef.current === targetUserId &&
						userSessionRef.current === targetSession &&
						currentSessionIdentityRef.current === sessionIdentity
					)
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
		baselineKnown,
		isVerifying,
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
			isVerifying ||
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
			if (
				currentUserIdRef.current !== targetUserId ||
				userSessionRef.current !== targetSession ||
				currentSessionIdentityRef.current !== sessionIdentity
			)
				return false;
			setSaveFeedback({
				kind: "outcome_unknown",
				verification: "unavailable",
				diagnostics: saveFeedback?.diagnostics ?? {},
			});
			setError(formatError(verificationError));
			return false;
		} finally {
			if (
				currentUserIdRef.current === targetUserId &&
				userSessionRef.current === targetSession &&
				currentSessionIdentityRef.current === sessionIdentity
			)
				setIsVerifying(false);
		}
	}, [
		acceptProfile,
		draft,
		formatError,
		isLoaded,
		isSaving,
		isVerifying,
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
