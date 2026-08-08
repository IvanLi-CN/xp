import type {
	AdminIpGeoSource,
	AdminIpUsageWarning,
} from "../api/adminIpUsage";
import { alertClass } from "./ui-helpers";

export function IpUsageWarningList({
	warnings,
}: {
	warnings: AdminIpUsageWarning[];
}) {
	if (warnings.length === 0) return null;
	return (
		<div className="space-y-2">
			{warnings.map((warning) => (
				<div
					key={warning.code}
					className={alertClass(
						warning.code === "online_stats_unavailable" ? "warning" : "info",
					)}
				>
					<span>{warning.message}</span>
				</div>
			))}
		</div>
	);
}

export function IpGeoSourceNotice({
	geoSource,
}: {
	geoSource?: AdminIpGeoSource;
}) {
	if (!geoSource) return null;
	const message = (() => {
		switch (geoSource) {
			case "country_is":
				return "Geo enrichment uses the free country.is hosted API.";
			case "managed_dbip_lite":
				return "Geo enrichment uses legacy managed DB-IP Lite MMDB data.";
			case "external_override":
				return "Geo enrichment uses a legacy external MMDB override.";
			case "missing":
				return "Geo enrichment is disabled (set XP_IP_GEO_ENABLED=true to enable country.is lookups).";
		}
	})();
	return (
		<div className={alertClass("info", "py-2 text-sm")}>
			<span>{message}</span>
		</div>
	);
}
