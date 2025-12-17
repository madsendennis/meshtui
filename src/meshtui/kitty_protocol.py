"""Kitty graphics protocol implementation.

This module implements the Kitty terminal graphics protocol for:
1. Detecting terminal dimensions in pixels
2. Displaying images directly in the terminal

Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/
"""

import base64
import fcntl
import os
import select
import sys
import termios
import tty

# Cache the detection result to avoid multiple queries
_kitty_protocol_supported = None
_terminal_bg_is_light = None
_terminal_bg_color: tuple[int, int, int] | None = None


def get_terminal_bg_ansi() -> str:
    """Get ANSI escape code for terminal background color.

    Returns:
        ANSI escape code string (e.g. "\033[48;2;0;0;0m")
    """
    detect_terminal_background()  # Ensure color is detected
    if _terminal_bg_color:
        r, g, b = _terminal_bg_color
        return f"\033[48;2;{r};{g};{b}m"
    # Fallback: Black for dark, White for light
    if _terminal_bg_is_light:
        return "\033[48;2;255;255;255m"  # White RGB
    return "\033[48;2;0;0;0m"  # Black RGB


def detect_terminal_background() -> bool:
    """Detect if terminal has a light background.

    Uses OSC 11 query to get background color from terminal.
    Falls back to assuming dark background if detection fails.

    Returns:
        True if background is light, False if dark or unknown
    """
    global _terminal_bg_is_light, _terminal_bg_color

    # Return cached result
    if _terminal_bg_is_light is not None:
        return _terminal_bg_is_light

    if not sys.stdout.isatty():
        _terminal_bg_is_light = False
        return False

    try:
        fd = sys.stdin.fileno()
        old_settings = termios.tcgetattr(fd)
        old_flags = fcntl.fcntl(fd, fcntl.F_GETFL)

        try:
            tty.setraw(fd)
            fcntl.fcntl(fd, fcntl.F_SETFL, old_flags | os.O_NONBLOCK)

            # Query terminal background color using OSC 11
            query = "\033]11;?\033\\"
            sys.stdout.write(query)
            sys.stdout.flush()

            # Wait for response
            if select.select([sys.stdin], [], [], 0.2)[0]:
                try:
                    response = os.read(fd, 1024).decode("utf-8", errors="ignore")
                    # Response format: \033]11;rgb:RRRR/GGGG/BBBB\033\\
                    if "rgb:" in response:
                        # Extract RGB values
                        rgb_part = response.split("rgb:")[1].split("\033")[0]
                        r, g, b = rgb_part.split("/")
                        # Convert hex to int (take first 2 chars of 4-char hex)
                        r_val = int(r[:2], 16)
                        g_val = int(g[:2], 16)
                        b_val = int(b[:2], 16)

                        _terminal_bg_color = (r_val, g_val, b_val)

                        # Calculate luminance (perceived brightness)
                        luminance = 0.299 * r_val + 0.587 * g_val + 0.114 * b_val
                        is_light = luminance > 128
                        _terminal_bg_is_light = is_light
                        return is_light
                except (OSError, ValueError, IndexError):
                    pass

            # Default to dark background
            _terminal_bg_is_light = False
            return False

        finally:
            fcntl.fcntl(fd, fcntl.F_SETFL, old_flags)
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)

    except (OSError, termios.error):
        _terminal_bg_is_light = False
        return False


def is_kitty_terminal() -> bool:
    """Check if terminal supports Kitty graphics protocol.

    Queries the terminal for graphics protocol support by sending a
    detection query and checking for a valid response.

    Returns:
        True if the terminal supports Kitty graphics protocol
    """
    global _kitty_protocol_supported

    # Return cached result if available
    if _kitty_protocol_supported is not None:
        return _kitty_protocol_supported

    # Quick check: if not a TTY, definitely not supported
    if not sys.stdout.isatty():
        _kitty_protocol_supported = False
        return False

    try:
        # Save terminal settings
        fd = sys.stdin.fileno()
        old_settings = termios.tcgetattr(fd)
        old_flags = fcntl.fcntl(fd, fcntl.F_GETFL)

        try:
            # Set terminal to raw mode to read response
            tty.setraw(fd)
            # Make stdin non-blocking
            fcntl.fcntl(fd, fcntl.F_SETFL, old_flags | os.O_NONBLOCK)

            # Send a query for graphics protocol support
            # Using a query with id=1, quiet=1 (suppress error messages)
            # The terminal should respond if it supports the protocol
            query = "\033_Gi=1,a=q;\033\\"
            sys.stdout.write(query)
            sys.stdout.flush()

            # Wait for response with timeout
            if select.select([sys.stdin], [], [], 0.1)[0]:
                # Read response (non-blocking)
                try:
                    response = os.read(fd, 1024)
                    # A valid Kitty graphics response starts with \033_G
                    result = b"\033_G" in response
                    _kitty_protocol_supported = result
                    return result
                except OSError:
                    pass

            _kitty_protocol_supported = False
            return False

        finally:
            # Restore terminal settings and flags
            fcntl.fcntl(fd, fcntl.F_SETFL, old_flags)
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)

    except (OSError, termios.error, ValueError):
        # If we can't query (not a TTY, permissions, etc.), assume not supported
        _kitty_protocol_supported = False
        return False


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
            "Terminal does not support the Kitty graphics protocol. "
            "Please use a compatible terminal such as Kitty, Ghostty, or WezTerm."
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


def display_image(image_data: bytes, width: int, height: int, cols: int = 0, rows: int = 0) -> None:
    """Display an image in the terminal using Kitty graphics protocol.

    Args:
        image_data: PNG image data as bytes
        width: Image width in pixels (optional, for protocol metadata)
        height: Image height in pixels (optional, for protocol metadata)
        cols: Number of columns to fill (optional)
        rows: Number of rows to fill (optional)

    Raises:
        RuntimeError: If not running in a Kitty-compatible terminal
    """
    if not is_kitty_terminal():
        raise RuntimeError(
            "Terminal does not support the Kitty graphics protocol. Cannot display images."
        )

    # Encode image data to base64
    encoded = base64.b64encode(image_data).decode("ascii")

    # Split into chunks (Kitty protocol supports chunking for large images)
    chunk_size = 4096
    chunks = [encoded[i : i + chunk_size] for i in range(0, len(encoded), chunk_size)]

    # Send the image using Kitty graphics protocol
    # Format: ESC _G<control_data>;<payload>ESC \
    # Parameters:
    #   a=T - transmit image data
    #   f=100 - PNG format (100 = direct RGB data, but we use PNG)
    #   t=d - direct data (base64-encoded PNG)
    #   c, r - columns and rows to fill
    for i, chunk in enumerate(chunks):
        if i == 0:
            # First chunk - include all control data
            # z=-1 places image below text
            control = "a=T,f=100,t=d,z=-1"

            # Add scaling parameters if provided
            if cols > 0 and rows > 0:
                control += f",c={cols},r={rows}"

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

    # No newline after image to prevent scrolling in TUI mode
    # print()


def clear_images() -> None:
    """Clear all Kitty graphics images from the terminal."""
    # Delete all images: a=d (delete), d=a (all images)
    sys.stdout.write("\033_Ga=d,d=a;\033\\")
    sys.stdout.flush()
