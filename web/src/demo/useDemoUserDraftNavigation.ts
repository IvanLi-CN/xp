import type { Dispatch, SetStateAction } from "react";

import { useObjectNavigationDirtySections } from "../components/ObjectNavigationGuard";
import type { ToastVariant } from "../components/Toast";
import type { DemoUser } from "./types";

type SetValue<Value> = Dispatch<SetStateAction<Value>>;

type Props = {
	userId: string;
	user: DemoUser | undefined;
	canWrite: boolean;
	defaultLimitGb: number | null;
	updateUser: (
		userId: string,
		patch: Partial<
			Pick<
				DemoUser,
				| "displayName"
				| "locale"
				| "tier"
				| "quotaLimitGb"
				| "endpointIds"
				| "mihomoMixinYaml"
				| "mihomoExtraProxiesYaml"
				| "mihomoExtraProxyProvidersYaml"
			>
		>,
	) => void;
	pushToast: (toast: { variant: ToastVariant; message: string }) => void;
	draft: {
		displayName: string;
		resetPolicy: "monthly" | "unlimited";
		tier: DemoUser["tier"];
		locale: string;
		selectedIds: string[];
		mihomoMixinYaml: string;
		mihomoExtraProxiesYaml: string;
		mihomoExtraProxyProvidersYaml: string;
	};
	setters: {
		setDisplayName: SetValue<string>;
		setResetPolicy: SetValue<"monthly" | "unlimited">;
		setResetDay: SetValue<number>;
		setResetTzOffsetMinutes: SetValue<number>;
		setTier: SetValue<DemoUser["tier"]>;
		setLocale: SetValue<string>;
		setSelectedIds: SetValue<string[]>;
		setMihomoMixinYaml?: SetValue<string>;
		setMihomoExtraProxiesYaml?: SetValue<string>;
		setMihomoExtraProxyProvidersYaml?: SetValue<string>;
	};
	mihomoProfile?: {
		dirty: boolean;
		isBusy?: () => boolean;
		save: () => Promise<boolean>;
		discard: () => void;
	};
};

function normalizedIds(ids: string[]) {
	return [...new Set(ids)].sort().join("|");
}

export function useDemoUserDraftNavigation({
	userId,
	user,
	canWrite,
	defaultLimitGb,
	updateUser,
	pushToast,
	draft,
	setters,
	mihomoProfile,
}: Props) {
	const profileDirty =
		user !== undefined &&
		(draft.displayName !== user.displayName ||
			draft.locale !== user.locale ||
			draft.tier !== user.tier ||
			(draft.resetPolicy === "unlimited") !== (user.quotaLimitGb === null));
	const accessDirty =
		user !== undefined &&
		normalizedIds(draft.selectedIds) !== normalizedIds(user.endpointIds);
	const mihomoDirty = mihomoProfile
		? mihomoProfile.dirty
		: user !== undefined &&
			(draft.mihomoMixinYaml !== user.mihomoMixinYaml ||
				draft.mihomoExtraProxiesYaml !== user.mihomoExtraProxiesYaml ||
				draft.mihomoExtraProxyProvidersYaml !==
					user.mihomoExtraProxyProvidersYaml);

	function saveProfile(): Promise<boolean> {
		if (!user || !canWrite) return Promise.resolve(false);
		updateUser(user.id, {
			displayName: draft.displayName,
			locale: draft.locale,
			tier: draft.tier,
			quotaLimitGb:
				draft.resetPolicy === "unlimited"
					? null
					: (user.quotaLimitGb ?? defaultLimitGb ?? 100),
		});
		pushToast({ variant: "success", message: "User saved." });
		return Promise.resolve(true);
	}

	function discardProfile() {
		if (!user) return;
		setters.setDisplayName(user.displayName);
		setters.setResetPolicy(
			user.quotaLimitGb === null ? "unlimited" : "monthly",
		);
		setters.setResetDay(1);
		setters.setResetTzOffsetMinutes(0);
		setters.setTier(user.tier);
		setters.setLocale(user.locale);
	}

	function saveAccess(): Promise<boolean> {
		if (!user || !canWrite) return Promise.resolve(false);
		updateUser(user.id, { endpointIds: draft.selectedIds });
		pushToast({ variant: "success", message: "Access saved." });
		return Promise.resolve(true);
	}

	function discardAccess() {
		if (user) setters.setSelectedIds(user.endpointIds);
	}

	function saveMihomoProfile(): Promise<boolean> {
		if (mihomoProfile) return mihomoProfile.save();
		if (!user || !canWrite) return Promise.resolve(false);
		updateUser(user.id, {
			mihomoMixinYaml: draft.mihomoMixinYaml,
			mihomoExtraProxiesYaml: draft.mihomoExtraProxiesYaml,
			mihomoExtraProxyProvidersYaml: draft.mihomoExtraProxyProvidersYaml,
		});
		pushToast({ variant: "success", message: "Mihomo profile saved." });
		return Promise.resolve(true);
	}

	function discardMihomoProfile() {
		if (mihomoProfile) {
			mihomoProfile.discard();
			return;
		}
		if (!user) return;
		setters.setMihomoMixinYaml?.(user.mihomoMixinYaml);
		setters.setMihomoExtraProxiesYaml?.(user.mihomoExtraProxiesYaml);
		setters.setMihomoExtraProxyProvidersYaml?.(
			user.mihomoExtraProxyProvidersYaml,
		);
	}

	useObjectNavigationDirtySections(`demo-user:${userId}`, [
		{
			id: "profile",
			label: "user profile",
			isDirty: () => profileDirty,
			save: saveProfile,
			discard: discardProfile,
		},
		{
			id: "access",
			label: "access",
			isDirty: () => accessDirty,
			save: saveAccess,
			discard: discardAccess,
		},
		{
			id: "mihomo-profile",
			label: "Mihomo profile",
			isDirty: () => mihomoDirty,
			isBusy: mihomoProfile?.isBusy,
			save: saveMihomoProfile,
			discard: discardMihomoProfile,
		},
	]);

	return {
		accessDirty,
		mihomoDirty,
		profileDirty,
		saveAccess,
		saveMihomoProfile,
		saveProfile,
	};
}
