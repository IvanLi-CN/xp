import unittest
from resource_summary import summarize


class ResourceTests(unittest.TestCase):
    def test_cpu_uses_load_interval_and_peak_memory_not_cleanup(self):
        samples = [
            dict(t=10, subject=dict(cpu_seconds=100, pss_kib=4096)),
            dict(t=11, subject=dict(cpu_seconds=100.5, pss_kib=16384)),
            dict(t=12, subject=dict(cpu_seconds=101, pss_kib=2048)),
        ]
        result = summarize(samples)
        self.assertEqual(result['cpu_percent'], 50)
        self.assertEqual(result['peak_pss_mib'], 16)
        self.assertEqual(result['sample_seconds'], 2)

    def test_missing_or_reset_sampling_is_not_zero_cpu(self):
        self.assertIsNone(summarize([])['cpu_percent'])
        with self.assertRaises(ValueError):
            summarize([dict(t=1, subject=dict(cpu_seconds=5, pss_kib=1)),
                       dict(t=2, subject=dict(cpu_seconds=1, pss_kib=1))])
