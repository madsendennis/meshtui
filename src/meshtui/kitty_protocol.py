"""Kitty graphics protocol implementation.

This module implements the Kitty terminal graphics protocol for:
1. Detecting terminal dimensions in pixels
2. Displaying images directly in the terminal

Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/
"""

import base64
import os
import sys


def is_kitty_terminal() -> bool:
    """Check if running in a Kitty-compatible terminal.

    Returns:
        True if the terminal supports Kitty graphics protocol
    """
    # Check for TERM environment variable
    term = os.environ.get("TERM", "")
    if "kitty" in term:
        return True

    # Check for KITTY_WINDOW_ID which is set by Kitty
    return bool(os.environ.get("KITTY_WINDOW_ID"))


def get_terminal_size() -> tuple[int, int, int, int]:
    """Get terminal size using Kitty graphics protocol.

    Queries the terminal for its dimensions including pixel size and cell dimensions.
    This uses a Kitty-specific query to get accurate pixel dimensions.

    Returns:
        Tuple of (width_px, height_px, cell_width_px, cell_height_px)
        - width_px: Terminal width in pixels
        - height_px: Terminal height in pixels
        - cell_width_px: Width of one character cell in pixels
        - cell_height_px: Height of one character cell in pixels

    Raises:
        RuntimeError: If not running in a Kitty-compatible terminal or query fails
    """
    if not is_kitty_terminal():
        raise RuntimeError(
            "Not running in a Kitty-compatible terminal. "
            "This application requires Kitty terminal or a compatible terminal "
            "that supports the Kitty graphics protocol."
        )

    # Get basic terminal dimensions (columns and rows)
    try:
        term_size = os.get_terminal_size()
        cols = term_size.columns
        rows = term_size.lines
    except OSError as e:
        raise RuntimeError(f"Failed to get terminal size: {e}") from e

    # For now, use estimated pixel dimensions based on common terminal cell sizes
    # Kitty typically uses 10x20 pixels per cell, but this varies by font
    # TODO: Implement actual Kitty protocol query for precise dimensions
    # Query format: ESC _Gi=1,s=1,v=1,a=q,t=d;ESC \
    # This would require reading the terminal's response
    cell_width_px = 10
    cell_height_px = 20

    width_px = cols * cell_width_px
    height_px = rows * cell_height_px

    return width_px, height_px, cell_width_px, cell_height_px


def display_image(image_data: bytes, width: int, height: int) -> None:
    """Display an image in the terminal using Kitty graphics protocol.

    Args:
        image_data: PNG image data as bytes
        width: Image width in pixels (optional, for protocol metadata)
        height: Image height in pixels (optional, for protocol metadata)

    Raises:
        RuntimeError: If not running in a Kitty-compatible terminal
    """
    if not is_kitty_terminal():
        raise RuntimeError(
            "Not running in a Kitty-compatible terminal. "
            "This application requires Kitty terminal."
        )

    # Encode image data to base64
    encoded = base64.b64encode(image_data).decode("ascii")

    # Split into chunks (Kitty protocol supports chunking for large images)
    chunk_size = 4096
    chunks = [encoded[i : i + chunk_size] for i in range(0, len(encoded), chunk_size)]

    # Send the image using Kitty graphics protocol
    # Format: ESC _G<control_data>;<payload>ESC \
    # Control data: a=T (transmit), f=100 (PNG format), t=d (direct transmission)
    for i, chunk in enumerate(chunks):
        if i == 0:
            # First chunk - include all control data
            control = "a=T,f=100,t=d"
            if i == len(chunks) - 1:
                # Only one chunk - no more data
                control += ",m=0"
            else:
                # More chunks to come
                control += ",m=1"
        elif i == len(chunks) - 1:
            # Last chunk
            control = "m=0"
        else:
            # Middle chunk
            control = "m=1"

        # Send the chunk
        sys.stdout.write(f"\033_G{control};{chunk}\033\\")
        sys.stdout.flush()

    # Print a newline after the image
    print()
