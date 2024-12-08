class Data:
    """
    The abstract base class for all data.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int
        """
        ...

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int
        """
        ...

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the `Data` class.

        Returns
        -------
        str
        """
        ...

    @classmethod
    def is_signal(cls, name: str = "") -> bool:
        """
        Determine if the current class is a signal type, optionally checking for a specific signal name.

        Parameters
        ----------
        name : str, optional
            The specific signal name to check.
            If `name` not provided or if an empty string is passed, the method checks whether the
            class name indicates a general signal type.
            If `name` is provided, the method checks if the class name corresponds to that specific signal.

        Returns
        -------
        bool
        """
        ...
