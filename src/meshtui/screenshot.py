"""Screenshot utilities for saving rendered images."""

from datetime import datetime
from pathlib import Path

from PIL import Image


def save_screenshot(
    image_data: bytes, width: int, height: int, output_dir: Path | None = None
) -> Path:
    """Save raw RGBA image data as a PNG file with timestamp.

    Args:
        image_data: Raw RGBA image data as bytes
        width: Image width in pixels
        height: Image height in pixels
        output_dir: Optional directory to save screenshot. If None, uses current working directory.

    Returns:
        Path to the saved screenshot file

    Raises:
        ValueError: If image_data size doesn't match width*height*4
        OSError: If file cannot be written
    """
    # Validate image data size
    expected_size = width * height * 4  # RGBA = 4 bytes per pixel
    if len(image_data) != expected_size:
        raise ValueError(
            f"Image data size mismatch: expected {expected_size} bytes, got {len(image_data)}"
        )

    # Generate timestamp filename
    timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    filename = f"screenshot-{timestamp}.png"

    # Determine output path
    if output_dir is None:
        output_dir = Path.cwd()
    output_path = output_dir / filename

    # Convert raw RGBA bytes to PIL Image
    img = Image.frombytes("RGBA", (width, height), image_data)

    # Save as PNG
    img.save(output_path, "PNG")

    return output_path
