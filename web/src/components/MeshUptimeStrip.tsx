import type { AdminMeshBucket } from "@/api/adminMesh";

type MeshUptimeStripProps = {
	buckets: AdminMeshBucket[];
	quality: "good" | "slow" | "unstable" | "down" | "unknown";
	label: string;
};

type Segment = {
	from: number;
	to: number;
	state: StripState;
	fallback: boolean;
};
type StripState = "good" | "degraded" | "down" | "unknown";

const stateColor: Record<StripState, string> = {
	good: "var(--color-success)",
	degraded: "var(--color-warning)",
	down: "var(--color-destructive)",
	unknown: "var(--color-muted)",
};

function bucketState(bucket: AdminMeshBucket): StripState {
	const success = bucket.mesh_success + bucket.public_success;
	const failure = bucket.mesh_failure + bucket.public_failure;
	if (success === 0 && failure === 0) return "unknown";
	if (success === 0) return "down";
	if (failure > 0) return "degraded";
	return "good";
}

function toSegments(buckets: AdminMeshBucket[]): Segment[] {
	if (buckets.length === 0) return [];
	const segments: Segment[] = [];
	buckets.forEach((bucket, index) => {
		const state = bucketState(bucket);
		const fallback = bucket.fallback_success > 0;
		const previous = segments.at(-1);
		if (previous?.state === state && previous.fallback === fallback) {
			previous.to = index + 1;
			return;
		}
		segments.push({ from: index, to: index + 1, state, fallback });
	});
	return segments;
}

export function MeshUptimeStrip({
	buckets,
	quality,
	label,
}: MeshUptimeStripProps) {
	const segments = toSegments(buckets);
	const width = Math.max(buckets.length, 1);
	const fallbackSegments = segments.filter((segment) => segment.fallback);
	const emptyColor =
		quality === "unknown"
			? stateColor.unknown
			: stateColor[quality === "down" ? "down" : "good"];

	return (
		<svg
			viewBox={`0 0 ${width} 24`}
			preserveAspectRatio="none"
			role="img"
			aria-label={label}
			className="h-7 w-full min-w-40 overflow-visible rounded-sm bg-muted/50"
		>
			{segments.length === 0 ? (
				<rect
					x="0"
					y="2"
					width={width}
					height="22"
					fill={emptyColor}
					opacity="0.32"
				/>
			) : (
				segments.map((segment) => (
					<rect
						key={`${segment.from}-${segment.to}-${segment.state}-${segment.fallback}`}
						x={segment.from}
						y="2"
						width={segment.to - segment.from}
						height="22"
						fill={stateColor[segment.state]}
						opacity={segment.state === "unknown" ? 0.3 : 0.78}
					/>
				))
			)}
			{fallbackSegments.map((segment) => (
				<rect
					key={`fallback-${segment.from}-${segment.to}`}
					x={segment.from}
					y="0"
					width={segment.to - segment.from}
					height="2"
					fill="var(--color-info)"
				/>
			))}
		</svg>
	);
}
