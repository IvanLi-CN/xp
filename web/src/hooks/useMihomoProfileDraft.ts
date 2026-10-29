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

export const MIHOMO_DRAFT_PERSISTENCE_WARNING =
	"Your Mihomo draft is kept in this tab, but browser storage is unavailable. " +
	"Keep this tab open or copy the draft before reloading.";

type UseMihomoProfileDraftProps = {
	clusterId?: string;
	userId: string;
	sessionKey?: string;
	profile: AdminUserMihomoProfile | undefined;
	readOnly?: boolean;
	saveProfile: (
		profile: AdminUserMihomoProfile,
		baseline: AdminUserMihomoProfile,
		allowOverwrite?: boolean,
	) => Promise<AdminUserMihomoProfile>;
	verifyProfile?: (
		profile: AdminUserMihomoProfile,
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
): boolean {
	if (typeof window === "undefined") return true;
	try {
		window.localStorage.setItem(
			storageKey,
			JSON.stringify({ draft, baseline, saveFeedback, updatedAt: Date.now() }),
		);
		return true;
	} catch {
		// Browser storage can be unavailable or full; the in-memory draft remains authoritative.
		return false;
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
	clusterId,
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
	const [loadedUserId, setLoadedUserId] = useState<string | null>(null);
	const incomingProfileKey = profileKey(profile);
	const isLoaded = loadedUserId === userId && profile !== undefined;
	const [isSaving, setIsSaving] = useState(false);
	const [isVerifying, setIsVerifying] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [saveFeedback, setSaveFeedback] = useState<MihomoSaveFeedback | null>(
		() => initialPersistedDraft?.saveFeedback ?? null,
	);
	const [storageError, setStorageError] = useState<string | null>(null);
	const [conflict, setConflict] = useState<MihomoProfileConflict | null>(null);
	const currentUserIdRef = useRef(userId);
	const sessionIdentity = `${clusterId ?? ""}\u0000${userId}\u0000${sessionKey ?? userId}`;
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
	const restoredSessionRef = useRef<string | null>(null);

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
		setStorageError(null);
		setConflict(null);
		restoredSessionRef.current = null;
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
			const recovery = readStoredDraft(clusterId, userId);
			const stored = recovery.value;
			setLoadedUserId(userId);
			if (recovery.error) setStorageError(recovery.error);
			setDraftState(cloneProfile(persisted?.draft ?? stored?.draft ?? profile));
			setBaseline(
				cloneProfile(persisted?.baseline ?? stored?.baseline ?? profile),
			);
			setBaselineKnown(
				persisted
					? persisted.baseline !== null
					: stored !== null || profile !== undefined,
			);
			if (stored && profileKey(stored.baseline) !== incomingProfileKey) {
				setConflict({
					server: cloneProfile(profile),
					draftBaseline: cloneProfile(stored.baseline),
				});
			} else {
				setConflict(null);
			}
			restoredSessionRef.current = sessionIdentity;
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
		setBaselineKnown(true);
		acceptedProfileKeyRef.current = incomingProfileKey;
		setError(null);
		setSaveFeedback(null);
		clearPersistedMihomoDraft(storageKey);
	}, [
		baseline,
		clusterId,
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
		const persisted = persistMihomoDraft(
			storageKey,
			draft,
			baselineKnown ? baseline : null,
			saveFeedback,
		);
		if (!persisted) setError(MIHOMO_DRAFT_PERSISTENCE_WARNING);
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
			setSaveFeedback((current) =>
				current?.verification === "matches" ? null : current,
			);
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
		setConflict(null);
		setStorageError(clearStoredDraft(clusterId, userId));
		clearPersistedMihomoDraft(storageKey);
	}, [
		baseline,
		clusterId,
		isSaving,
		isVerifying,
		loadedUserId,
		profile,
		storageKey,
		userId,
	]);

	const continueDraft = useCallback(() => {
		if (!conflict || isSaving) return;
		const nextBaseline = cloneProfile(conflict.server);
		setBaseline(nextBaseline);
		acceptedProfileKeyRef.current = profileKey(nextBaseline);
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

	const save = useCallback(
		async (allowOverwrite = false): Promise<boolean> => {
			if (
				readOnly ||
				!isLoaded ||
				isVerifying ||
				conflict ||
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
			const request = saveProfile(payload, baseline, allowOverwrite)
				.then((saved) => {
					if (
						currentUserIdRef.current !== targetUserId ||
						userSessionRef.current !== targetSession ||
						currentSessionIdentityRef.current !== sessionIdentity
					)
						return false;
					acceptProfile(saved, targetUserId);
					setSaveFeedback(null);
					setStorageError(clearStoredDraft(clusterId, targetUserId));
					clearPersistedMihomoDraft(storageKey);
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
		},
		[
			acceptProfile,
			clusterId,
			conflict,
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
			storageKey,
			userId,
		],
	);

	const verify = useCallback(async (): Promise<boolean> => {
		if (
			!verifyProfile ||
			readOnly ||
			!isLoaded ||
			conflict ||
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
		conflict,
		isLoaded,
		isSaving,
		isVerifying,
		readOnly,
		saveFeedback,
		sessionIdentity,
		userId,
		verifyProfile,
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
		isVerifying,
		saveFeedback,
		save,
		setField,
		verify,
		dirty,
		conflict,
		continueDraft,
		storageError,
		useServer,
	};
}
