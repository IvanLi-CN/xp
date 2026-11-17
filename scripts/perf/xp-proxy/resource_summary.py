"""Per-load resource deltas; never subtract a process-wide idle CPU counter."""


def summarize(samples):
    usable = [s for s in samples if s.get('subject')]
    if len(usable) < 2:
        return dict(cpu_percent=None, peak_pss_mib=None, sample_seconds=0)
    first, last = usable[0], usable[-1]
    seconds = last['t'] - first['t']
    cpu = last['subject']['cpu_seconds'] - first['subject']['cpu_seconds']
    if seconds <= 0 or cpu < 0:
        raise ValueError('invalid resource sampling interval or process identity reset')
    return dict(
        cpu_percent=cpu / seconds * 100,
        peak_pss_mib=max(s['subject']['pss_kib'] for s in usable) / 1024,
        sample_seconds=seconds,
    )
