import { useQuery } from "@tanstack/react-query";

import { fetchHealth } from "../api/health";
import { Button } from "../components/Button";

export function HomePage() {
	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	return (
		<div className="space-y-6">
			<div>
				<h1 className="text-2xl font-bold">xp</h1>
				<p className="text-sm opacity-70">Control plane bootstrap UI.</p>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Backend health</h2>
					{health.isLoading ? (
						<p>Loading...</p>
					) : health.isError ? (
						<p className="text-error">Failed to reach backend.</p>
					) : (
						<p>
							Status:{" "}
							<span className="font-mono">
								{health.data?.status ?? "unknown"}
							</span>
						</p>
					)}
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							loading={health.isFetching}
							onClick={() => health.refetch()}
						>
							Refresh
						</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
