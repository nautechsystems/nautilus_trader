# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

import os


def get_api_key() -> str | None:
    """
    Get the Alpaca API key from environment variables.

    Returns
    -------
    str or None
        The API key from APCA_API_KEY_ID env var, or None if not set.

    """
    return os.environ.get("APCA_API_KEY_ID")


def get_api_secret() -> str | None:
    """
    Get the Alpaca API secret from environment variables.

    Returns
    -------
    str or None
        The API secret from APCA_API_SECRET_KEY env var, or None if not set.

    """
    return os.environ.get("APCA_API_SECRET_KEY")


def get_access_token() -> str | None:
    """
    Get the Alpaca OAuth access token from environment variables.

    Returns
    -------
    str or None
        The access token from APCA_API_ACCESS_TOKEN env var, or None if not set.

    """
    return os.environ.get("APCA_API_ACCESS_TOKEN")


def get_base_url() -> str | None:
    """
    Get the Alpaca API base URL from environment variables.

    Returns
    -------
    str or None
        The base URL from APCA_API_BASE_URL env var, or None if not set.

    """
    return os.environ.get("APCA_API_BASE_URL")


def resolve_credentials(
    api_key: str | None = None,
    api_secret: str | None = None,
    access_token: str | None = None,
) -> tuple[str | None, str | None, str | None]:
    """
    Resolve credentials from provided values or environment variables.

    Priority:
    1. OAuth access_token (if provided or in env)
    2. API key/secret pair

    Parameters
    ----------
    api_key : str, optional
        The API key (overrides env var).
    api_secret : str, optional
        The API secret (overrides env var).
    access_token : str, optional
        The OAuth access token (overrides env var).

    Returns
    -------
    tuple[str | None, str | None, str | None]
        Tuple of (api_key, api_secret, access_token).

    """
    resolved_access_token = access_token or get_access_token()
    resolved_api_key = api_key or get_api_key()
    resolved_api_secret = api_secret or get_api_secret()

    return resolved_api_key, resolved_api_secret, resolved_access_token


def get_auth_headers(
    api_key: str | None = None,
    api_secret: str | None = None,
    access_token: str | None = None,
) -> dict[str, str]:
    """
    Build authentication headers for Alpaca API requests.

    OAuth access_token takes precedence over API key/secret if both are provided.

    Parameters
    ----------
    api_key : str, optional
        The API key.
    api_secret : str, optional
        The API secret.
    access_token : str, optional
        The OAuth access token.

    Returns
    -------
    dict[str, str]
        Headers dict with appropriate authentication.

    Raises
    ------
    ValueError
        If no valid credentials are provided.

    """
    resolved_key, resolved_secret, resolved_token = resolve_credentials(
        api_key, api_secret, access_token
    )

    # OAuth takes precedence
    if resolved_token:
        return {"Authorization": f"Bearer {resolved_token}"}

    # Fall back to API key/secret
    if resolved_key and resolved_secret:
        return {
            "APCA-API-KEY-ID": resolved_key,
            "APCA-API-SECRET-KEY": resolved_secret,
        }

    raise ValueError(
        "No valid Alpaca credentials found. "
        "Provide api_key/api_secret or access_token, "
        "or set APCA_API_KEY_ID/APCA_API_SECRET_KEY or APCA_API_ACCESS_TOKEN env vars."
    )

