from nautilus_trader.common.providers import InstrumentProvider


# The 'pragma: no cover' comment excludes a method from test coverage.
# https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html
# The reason for their use is to reduce redundant/needless tests which simply
# assert that a `NotImplementedError` is raised when calling abstract methods.
# These tests are expensive to maintain (as they must be kept in line with any
# refactorings), and offer little to no benefit in return. The intention
# is for all method implementations to be fully covered by tests.

# *** THESE PRAGMA: NO COVER COMMENTS MUST BE REMOVED IN ANY IMPLEMENTATION. ***


class TemplateInstrumentProvider(InstrumentProvider):
    """
    An example template of an ``InstrumentProvider`` showing the minimal methods which
    must be implemented for an integration to be complete.

    The base class provides default implementations for ``load_ids_async`` and
    ``load_async`` that delegate to ``load_all_async`` with filtering. Override
    those methods only if the venue API supports per-instrument fetching.

    """

    async def load_all_async(
        self,
        filters: dict | None = None,
    ) -> None:
        raise NotImplementedError(
            "method `load_all_async` must be implemented in the subclass",
        )  # pragma: no cover
