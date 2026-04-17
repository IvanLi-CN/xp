import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	fetchAdminClusterSettings,
	putAdminClusterSettings,
} from "../api/adminClusterSettings";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Label } from "../components/ui/label";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

type ClusterSettingsDraft = {
	ipGeoEnabled: boolean;
	ipGeoOrigin: string;
};

function toDraft(input: {
	ip_geo_enabled: boolean;
	ip_geo_origin: string;
}): ClusterSettingsDraft {
	return {
		ipGeoEnabled: input.ip_geo_enabled,
		ipGeoOrigin: input.ip_geo_origin,
	};
}

export function ClusterSettingsPage() {
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();
	const [draft, setDraft] = useState<ClusterSettingsDraft | null>(null);
	const [baseline, setBaseline] = useState<ClusterSettingsDraft | null>(null);

	const settingsQuery = useQuery({
		queryKey: ["adminClusterSettings", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminClusterSettings(adminToken, signal),
	});

	useEffect(() => {
		if (!settingsQuery.data) return;
		const next = toDraft(settingsQuery.data);
		setDraft(next);
		setBaseline(next);
	}, [settingsQuery.data]);

	const isDirty = useMemo(() => {
		if (!draft || !baseline) return false;
		return (
			draft.ipGeoEnabled !== baseline.ipGeoEnabled ||
			draft.ipGeoOrigin !== baseline.ipGeoOrigin
		);
	}, [baseline, draft]);

	const saveMutation = useMutation({
		mutationFn: async (next: ClusterSettingsDraft) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			return putAdminClusterSettings(adminToken, {
				ip_geo_enabled: next.ipGeoEnabled,
				ip_geo_origin: next.ipGeoOrigin,
			});
		},
		onSuccess: (saved) => {
			queryClient.setQueryData(["adminClusterSettings", adminToken], saved);
			const next = toDraft(saved);
			setDraft(next);
			setBaseline(next);
			pushToast({ variant: "success", message: "Cluster settings saved." });
		},
		onError: (error) => {
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	const headerActions = (
		<Button
			variant="secondary"
			size="sm"
			loading={settingsQuery.isFetching}
			disabled={adminToken.length === 0}
			onClick={() => settingsQuery.refetch()}
		>
			Refresh
		</Button>
	);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="需要管理员 Token"
					description="请先在 Dashboard 页面设置 admin token，再查看集群设置。"
				/>
			);
		}

		if (settingsQuery.isError) {
			return (
				<PageState
					variant="error"
					title="加载失败"
					description={formatErrorMessage(settingsQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={settingsQuery.isFetching}
							onClick={() => settingsQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		if (settingsQuery.isLoading || !draft || !baseline || !settingsQuery.data) {
			return (
				<PageState
					variant="loading"
					title="正在加载集群设置"
					description="正在读取当前集群级 IP Geo 配置。"
				/>
			);
		}

		return (
			<div className="space-y-4">
				<div className="xp-card">
					<div className="xp-card-body space-y-4">
						<div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
							<div>
								<h2 className="text-base font-semibold">Inbound IP Geo</h2>
								<p className="text-sm text-muted-foreground">
									集群级控制 inbound IP geo enrichment。保存后由 Raft
									统一下发，不再需要逐节点改环境变量。
								</p>
							</div>
							<div className="rounded-2xl border border-border/70 bg-card px-3 py-2 text-sm text-muted-foreground">
								{settingsQuery.data.legacy_fallback_in_use
									? "Current value comes from the leader's legacy env fallback."
									: "Cluster state is active."}
							</div>
						</div>

						<div className="rounded-2xl border border-border/70 bg-card px-4 py-4 shadow-sm">
							<div className="flex items-start gap-3">
								<Checkbox
									id="cluster-ip-geo-enabled"
									checked={draft.ipGeoEnabled}
									onCheckedChange={(checked) =>
										setDraft((current) =>
											current
												? {
														...current,
														ipGeoEnabled: checked === true,
													}
												: current,
										)
									}
									aria-describedby="cluster-ip-geo-enabled-hint"
								/>
								<div className="space-y-2">
									<Label htmlFor="cluster-ip-geo-enabled">
										Enable IP geo enrichment
									</Label>
									<p
										id="cluster-ip-geo-enabled-hint"
										className="text-sm text-muted-foreground"
									>
										Use country.is lookups to annotate inbound IP reports with
										region and operator data.
									</p>
								</div>
							</div>
						</div>

						<div className="space-y-2">
							<Label htmlFor="cluster-ip-geo-origin">country.is origin</Label>
							<Input
								id="cluster-ip-geo-origin"
								value={draft.ipGeoOrigin}
								onChange={(event) =>
									setDraft((current) =>
										current
											? { ...current, ipGeoOrigin: event.target.value }
											: current,
									)
								}
								placeholder="https://api.country.is"
								autoCapitalize="none"
								autoCorrect="off"
								spellCheck={false}
							/>
							<p className="text-sm text-muted-foreground">
								Leave empty to use <code>https://api.country.is</code>. Only
								absolute <code>http(s)</code> URLs are accepted.
							</p>
						</div>

						<div className="flex flex-wrap items-center gap-3">
							<Button
								variant="primary"
								disabled={!isDirty}
								loading={saveMutation.isPending}
								onClick={() => draft && saveMutation.mutate(draft)}
							>
								Save
							</Button>
							<Button
								variant="secondary"
								disabled={!isDirty || saveMutation.isPending}
								onClick={() => setDraft(baseline)}
							>
								Reset
							</Button>
						</div>
					</div>
				</div>

				<div className="rounded-2xl border border-border/70 bg-card px-4 py-3 text-sm text-muted-foreground shadow-sm">
					Local process diagnostics remain available in{" "}
					<Link className="xp-link" to="/service-config">
						Service config
					</Link>
					. Legacy <code>XP_IP_GEO_ENABLED</code> and{" "}
					<code>XP_IP_GEO_ORIGIN</code> now act only as bootstrap fallback
					before this page is saved for the first time.
				</div>
			</div>
		);
	})();

	return (
		<div className="space-y-5">
			<PageHeader
				title="Cluster settings"
				description="Cluster-wide controls replicated through Raft."
				actions={headerActions}
			/>
			{content}
		</div>
	);
}
