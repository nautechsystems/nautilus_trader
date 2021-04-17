import pytest


class PerformanceTestCase:
    @pytest.fixture(autouse=True)
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def setup(self, benchmark):
        self.benchmark = benchmark
