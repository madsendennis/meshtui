"""Kitty graphics protocol implementation."""


def get_terminal_size() -> tuple[int, int]:
    """Get terminal size in pixels using Kitty graphics protocol.

    Returns:
        Tuple of (width, height) in pixels

    Raises:
        RuntimeError: If not running in a Kitty-compatible terminal
    """
    # TODO: Implement Kitty protocol query
    raise NotImplementedError("Terminal size detection not yet implemented")


def display_image(image_data: bytes, width: int, height: int) -> None:
    """Display an image in the terminal using Kitty graphics protocol.

    Args:
        image_data: PNG image data as bytes
        width: Image width in pixels
        height: Image height in pixels
    """
    # TODO: Implement Kitty protocol image display
    raise NotImplementedError("Image display not yet implemented")
