"""Configuration loader for meshtui rendering settings."""

import tomllib
from pathlib import Path
from typing import Any, cast


def _deep_merge(base: dict[str, Any], override: dict[str, Any]) -> dict[str, Any]:
    """Deep merge override dict into base dict.

    Args:
        base: Base configuration dictionary
        override: Override configuration dictionary

    Returns:
        Merged configuration dictionary
    """
    result = base.copy()
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = _deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def load_config(user_config_path: Path | None = None) -> dict[str, Any]:
    """Load rendering configuration from default and optional user config.

    Always loads default_config.toml first, then merges in user config if provided.
    User config only needs to specify values they want to override.

    Args:
        user_config_path: Optional path to user config file

    Returns:
        Dictionary containing merged configuration settings

    Raises:
        RuntimeError: If default config file cannot be loaded
    """
    # Load default config
    default_config_path = Path(__file__).parent / "default_config.toml"

    try:
        with open(default_config_path, "rb") as f:
            config = tomllib.load(f)
    except Exception as e:
        raise RuntimeError(f"Failed to load default config from {default_config_path}: {e}") from e

    # Merge user config if provided
    if user_config_path and user_config_path.exists():
        try:
            with open(user_config_path, "rb") as f:
                user_config = tomllib.load(f)
            config = _deep_merge(config, user_config)
        except Exception as e:
            # Log warning but continue with default config
            print(f"Warning: Failed to load user config from {user_config_path}: {e}")

    return config


# Load config once at module import (default only)
_CONFIG = load_config()


def get_material_config() -> dict[str, Any]:
    """Get material configuration settings."""
    return cast(dict[str, Any], _CONFIG["material"])


def get_scene_config() -> dict[str, Any]:
    """Get scene configuration settings."""
    return cast(dict[str, Any], _CONFIG["scene"])


def get_lighting_config() -> dict[str, Any]:
    """Get lighting configuration settings."""
    return cast(dict[str, Any], _CONFIG["lighting"])


def get_wireframe_config() -> dict[str, Any]:
    """Get wireframe configuration settings."""
    return cast(dict[str, Any], _CONFIG["wireframe"])


def get_camera_config() -> dict[str, Any]:
    """Get camera configuration settings."""
    return cast(dict[str, Any], _CONFIG["camera"])


def get_view_config() -> dict[str, Any]:
    """Get view configuration settings."""
    return cast(dict[str, Any], _CONFIG["view"])


def get_orbital_camera_config() -> dict[str, Any]:
    """Get orbital camera configuration settings."""
    return cast(dict[str, Any], _CONFIG["orbital_camera"])


def get_performance_config() -> dict[str, Any]:
    """Get performance configuration settings."""
    return cast(dict[str, Any], _CONFIG["performance"])
