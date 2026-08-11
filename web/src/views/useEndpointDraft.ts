import type { Dispatch, SetStateAction } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type AdminEndpoint,
	type MihomoSmuxConfig,
	type VlessRealityTransport,
	parseMihomoSmuxConfig,
	parseVlessRealityTransport,
} from "../api/adminEndpoints";
import { useObjectNavigationDirtySections } from "../components/ObjectNavigationGuard";
import { normalizeAcceptedAuthority } from "../utils/acceptedAuthority";
import {
	type CanaryUpstreamMode,
	arraysEqual,
	authoritySetsEqual,
	parseVlessMeta,
} from "../utils/endpointDetailsVless";
import { normalizeRealityServerName } from "../utils/realityServerName";

type SetValue<Value> = Dispatch<SetStateAction<Value>>;

type EndpointDraftState = {
	port: string;
	realityDest: string;
	realityServerNamesManual: string[];
	realityFingerprint: string;
	upstreamUrl: string;
	upstreamMode: CanaryUpstreamMode;
	acceptedAuthorities: string[];
	mihomoSmux: MihomoSmuxConfig;
	mihomoSmuxMaxConnections: string;
	mihomoSmuxMinStreams: string;
};

type EndpointDraftSetters = {
	setPort: SetValue<string>;
	setRealityDest: SetValue<string>;
	setRealityServerNamesManual: SetValue<string[]>;
	setRealityFingerprint: SetValue<string>;
	setUpstreamUrl: SetValue<string>;
	setUpstreamMode: SetValue<CanaryUpstreamMode>;
	setAcceptedAuthorities: SetValue<string[]>;
	setMihomoSmux: SetValue<MihomoSmuxConfig>;
	setMihomoSmuxMaxConnections: SetValue<string>;
	setMihomoSmuxMinStreams: SetValue<string>;
};

function hydrateEndpointDraft(
	endpoint: AdminEndpoint,
	setters: EndpointDraftSetters,
) {
	setters.setPort(String(endpoint.port));
	const smux = parseMihomoSmuxConfig(endpoint.meta.mihomo_smux);
	setters.setMihomoSmux(smux);
	setters.setMihomoSmuxMaxConnections(String(smux.max_connections));
	setters.setMihomoSmuxMinStreams(String(smux.min_streams));
	if (endpoint.kind === "vless_reality_vision_tcp") {
		const snapshot = parseVlessMeta(endpoint.meta);
		setters.setRealityDest(snapshot.realityDest);
		setters.setRealityServerNamesManual(snapshot.realityServerNames);
		setters.setRealityFingerprint(snapshot.realityFingerprint);
		setters.setUpstreamUrl(snapshot.canaryUpstreamUrl);
		setters.setUpstreamMode(snapshot.canaryUpstreamMode);
		setters.setAcceptedAuthorities(snapshot.acceptedAuthorities);
		return;
	}
	setters.setRealityDest("");
	setters.setRealityServerNamesManual([]);
	setters.setRealityFingerprint("");
	setters.setUpstreamUrl("");
	setters.setUpstreamMode("auto");
	setters.setAcceptedAuthorities([]);
}

export function useEndpointDraft(
	endpoint: AdminEndpoint | undefined,
	draft: EndpointDraftState,
	setters: EndpointDraftSetters,
) {
	const [vlessTransport, setVlessTransport] =
		useState<VlessRealityTransport>("vision_tcp");
	const settersRef = useRef(setters);
	settersRef.current = setters;
	const discard = useCallback(() => {
		if (!endpoint) return;
		hydrateEndpointDraft(endpoint, settersRef.current);
		setVlessTransport(
			endpoint.kind === "vless_reality_vision_tcp"
				? parseVlessRealityTransport(endpoint.meta.transport)
				: "vision_tcp",
		);
	}, [endpoint]);

	useEffect(() => {
		discard();
	}, [discard]);
	const vlessTransportChanged =
		endpoint?.kind === "vless_reality_vision_tcp" &&
		vlessTransport !== parseVlessRealityTransport(endpoint.meta.transport);

	const isDirty = useMemo(() => {
		if (!endpoint || draft.port !== String(endpoint.port))
			return Boolean(endpoint);
		if (endpoint.kind === "ss2022_2022_blake3_aes_128_gcm") {
			const currentSmux = parseMihomoSmuxConfig(endpoint.meta.mihomo_smux);
			return (
				draft.mihomoSmux.enabled !== currentSmux.enabled ||
				draft.mihomoSmux.only_tcp !== currentSmux.only_tcp ||
				draft.mihomoSmux.max_connections !== currentSmux.max_connections ||
				draft.mihomoSmux.min_streams !== currentSmux.min_streams ||
				draft.mihomoSmuxMaxConnections !==
					String(currentSmux.max_connections) ||
				draft.mihomoSmuxMinStreams !== String(currentSmux.min_streams)
			);
		}
		const snapshot = parseVlessMeta(endpoint.meta);
		if (!snapshot.managedDefault) {
			const serverNames = draft.realityServerNamesManual
				.map(normalizeRealityServerName)
				.filter((value) => value.length > 0);
			return (
				vlessTransportChanged ||
				draft.realityDest.trim() !== snapshot.realityDest ||
				(draft.realityFingerprint.trim() || "chrome") !==
					snapshot.realityFingerprint ||
				!arraysEqual(serverNames, snapshot.realityServerNames)
			);
		}
		const authorities = draft.acceptedAuthorities
			.map(normalizeAcceptedAuthority)
			.filter((value) => value.length > 0);
		return (
			vlessTransportChanged ||
			draft.upstreamUrl.trim() !== snapshot.canaryUpstreamUrl ||
			draft.upstreamMode !== snapshot.canaryUpstreamMode ||
			!authoritySetsEqual(authorities, snapshot.acceptedAuthorities)
		);
	}, [draft, endpoint, vlessTransportChanged]);

	return {
		discard,
		isDirty,
		vlessTransport,
		setVlessTransport,
		vlessTransportChanged,
	};
}

export function useEndpointDraftNavigation(
	endpointId: string,
	isDirty: boolean,
	mutation: { mutateAsync: () => Promise<unknown> },
	discard: () => void,
) {
	useObjectNavigationDirtySections(`endpoint:${endpointId}`, [
		{
			id: "endpoint-config",
			label: "endpoint configuration",
			isDirty: () => isDirty,
			save: async () => {
				if (!isDirty) return true;
				try {
					await mutation.mutateAsync();
					return true;
				} catch {
					return false;
				}
			},
			discard,
		},
	]);
}
